-- The checkers screen, after Lidraughts: its light page, maple board,
-- "wide crown" pieces, square numbers and move list. Click one of your
-- pieces, then the square to move it to. For a double jump, click each
-- landing square in turn.

local storage = game:GetService("ReplicatedStorage")
local TweenService = game:GetService("TweenService")

local folder = storage:WaitForChild("TableGame")
local Screen = require(folder:WaitForChild("Screen"))
local Rules = require(folder:WaitForChild("Rules"))
local make, round, label = Screen.make, Screen.round, Screen.label

local LIGHT = Color3.fromRGB(224, 189, 147)
local DARK = Color3.fromRGB(180, 111, 59)
local LAST = Color3.fromRGB(170, 147, 35)
local PICKED = Color3.fromRGB(110, 98, 45)
local DOT = Color3.fromRGB(20, 85, 30)
local NUMBER = Color3.fromRGB(236, 214, 186)
local INK = Color3.fromRGB(77, 77, 77)
local SOFT = Color3.fromRGB(120, 120, 120)
local STRIPE = Color3.fromRGB(247, 246, 245)

-- The plain pieces, or the ones carved in wood.
local PIECE_SETS = { "", "wood-" }
local pieceSet = 1
local function pieceImage(piece)
	local name = ({ "black", "white", "black-king", "white-king" })[piece]
	return "libroxel://draughts-" .. PIECE_SETS[pieceSet] .. name
end

local ui = Screen.new(script.Parent, folder, {
	title = "Checkers",
	levels = Rules.LEVELS,
	level = 2,
	look = {
		background = Color3.fromRGB(237, 235, 233),
		page = Color3.fromRGB(255, 255, 255),
		ink = INK,
		soft = SOFT,
		line = Color3.fromRGB(217, 217, 217),
		plain = Color3.fromRGB(240, 238, 236),
		plainInk = INK,
		accent = Color3.fromRGB(54, 146, 231),
		go = Color3.fromRGB(98, 153, 36),
		danger = Color3.fromRGB(220, 50, 47),
		seat = STRIPE,
		turn = Color3.fromRGB(232, 242, 252),
		turnEdge = Color3.fromRGB(54, 146, 231),
		yourTurn = Color3.fromRGB(229, 240, 216),
		resultPage = Color3.fromRGB(255, 255, 255),
		resultEdge = Color3.fromRGB(98, 153, 36),
		waiting = Color3.fromRGB(199, 134, 42),
		toast = Color3.fromRGB(51, 51, 51),
		radius = 3,
	},
	pictureOf = function(seat)
		return pieceImage(seat)
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
		return (if seat == 1 then "Black" else "White") .. " · " .. men + kings .. (if kings > 0 then " (" .. kings .. " kings)" else "")
	end,
	sounds = {
		move = "libroxel://glass/move",
		capture = "libroxel://glass/capture",
		start = "libroxel://glass/notify",
		win = "libroxel://glass/victory",
		lose = "libroxel://glass/defeat",
		draw = "libroxel://glass/draw",
		challenge = "libroxel://glass/challenge",
	},
	rules = {
		"Black goes first. Pieces move one square diagonally forward.",
		"Jump an enemy piece to take it. Jumps are forced. Keep jumping if you can.",
		"Reach the far side to become a king. Kings move both ways.",
		"Take every piece or leave them stuck to win. 40 moves with no capture or pawn move is a draw.",
	},
})

local board = make("Frame", {
	Name = "Board",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.fromScale(0.5, 0.5),
	Size = UDim2.fromScale(1, 1),
	BackgroundColor3 = DARK,
	BorderSizePixel = 0,
}, ui.area)
make("UIAspectRatioConstraint", { AspectRatio = 1 }, board)

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
	local number = nil
	if dark then
		number = label(s, {
			Name = "Field",
			Position = UDim2.fromScale(0.05, 0.02),
			Size = UDim2.fromScale(0.4, 0.22),
			TextSize = 11,
			TextColor3 = NUMBER,
			Text = tostring(Rules.field(sq)),
			ZIndex = 2,
		})
	end
	local piece = make("ImageLabel", {
		Name = "Piece",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale(0.5, 0.5),
		Size = UDim2.fromScale(0.9, 0.9),
		BackgroundTransparency = 1,
		Visible = false,
		ZIndex = 3,
	}, s)
	local dot = make("Frame", {
		Name = "Dot",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale(0.5, 0.5),
		Size = UDim2.fromScale(0.28, 0.28),
		BackgroundColor3 = DOT,
		BackgroundTransparency = 0.5,
		Visible = false,
		ZIndex = 6,
	}, s)
	round(dot, 1000)
	squares[sq] = { button = s, piece = piece, dot = dot, dark = dark, number = number }
end

-- The move list, numbered in pairs the way Lidraughts shows it.
local extra = ui.extra
extra.Size = UDim2.new(1, -24, 1, -(150 + 2 * 54 + 116))
local piecesButton = make("TextButton", {
	Name = "Pieces",
	Size = UDim2.new(0, 120, 0, 26),
	BackgroundColor3 = Color3.fromRGB(255, 255, 255),
	TextColor3 = INK,
	TextSize = 13,
	Text = "Pieces: plain",
	AutoButtonColor = true,
}, extra)
round(piecesButton, 4)
local movesBox = make("ScrollingFrame", {
	Name = "Moves",
	Position = UDim2.fromOffset(0, 34),
	Size = UDim2.new(1, 0, 1, -34),
	BackgroundColor3 = Color3.fromRGB(255, 255, 255),
	BorderSizePixel = 0,
	ScrollBarThickness = 5,
	CanvasSize = UDim2.new(0, 0, 0, 0),
}, extra)
make("UIStroke", { Color = Color3.fromRGB(217, 217, 217), Thickness = 1 }, movesBox)

local shownMoves = -1
local function drawMoves(history)
	if #history == shownMoves then
		return
	end
	shownMoves = #history
	for _, child in movesBox:GetChildren() do
		if child:IsA("Frame") then
			child:Destroy()
		end
	end
	local rows = math.ceil(#history / 2)
	for r = 1, rows do
		local line = make("Frame", {
			Position = UDim2.new(0, 0, 0, (r - 1) * 26),
			Size = UDim2.new(1, -6, 0, 26),
			BackgroundTransparency = 1,
		}, movesBox)
		local index = make("Frame", { Size = UDim2.new(0, 38, 1, 0), BackgroundColor3 = STRIPE, BorderSizePixel = 0 }, line)
		label(index, { Size = UDim2.fromScale(1, 1), Text = tostring(r), TextSize = 13, TextColor3 = SOFT, TextXAlignment = Enum.TextXAlignment.Center })
		for k = 1, 2 do
			local text = history[r * 2 - 2 + k]
			if text then
				local latest = r * 2 - 2 + k == #history
				local cell = label(line, {
					Position = UDim2.new(0.5 * (k - 1), if k == 1 then 46 else 23, 0, 0),
					Size = UDim2.new(0.5, -23, 1, 0),
					Text = text,
					TextSize = 15,
					TextColor3 = INK,
					BackgroundColor3 = Color3.fromRGB(207, 225, 241),
					BackgroundTransparency = if latest then 0 else 1,
				})
				make("UIPadding", { PaddingLeft = UDim.new(0, 6) }, cell)
			end
		end
	end
	local height = rows * 26
	movesBox.CanvasSize = UDim2.new(0, 0, 0, height)
	movesBox.CanvasPosition = Vector2.new(0, math.max(0, height - movesBox.AbsoluteSize.Y))
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
	local targets = {}
	if v and ui.myTurn() and #path > 0 then
		for _, m in matching() do
			if m[#path + 1] then
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
		if table.find(path, sq) then
			colour = PICKED
		end
		s.button.BackgroundColor3 = colour
		if s.number then
			s.number.TextSize = math.max(9, math.floor(size / 8 * 0.14))
		end
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
			s.piece.Image = pieceImage(piece)
		end
		s.dot.Visible = targets[sq] == true
	end
	if v and v.status == "playing" then
		ui.subText = if v.game.mustJump and ui.myTurn() then "must jump" else nil
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
	ghost.Size = UDim2.fromScale(0.9 / 8, 0.9 / 8)
	ghost.ZIndex = 8
	s.piece.Visible = false
	TweenService:Create(ghost, TweenInfo.new(0.16, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
		Position = place(to) + UDim2.fromScale(1 / 16, 1 / 16),
	}):Play()
	task.delay(0.18, function()
		ghost:Destroy()
		draw()
	end)
end

ui.onTable = function(v, fresh)
	local before = shown
	shown = table.clone(v.game.board)
	if fresh then
		flipped = v.you == 2
		shownMoves = -1
	end
	path = {}
	draw()
	drawMoves(v.game.history or {})
	local last = v.game.last
	if not fresh and before and #last >= 2 and before[last[#last]] == 0 then
		slide(last[1], last[#last])
		ui.play(if math.abs(Rules.rowOf(last[1]) - Rules.rowOf(last[2])) == 2 then "capture" else "move")
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
				ui.toast("Keep jumping")
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
			ui.toast(if ui.view.game.mustJump then "You must jump" else "That piece can't move")
		end
		draw()
	end)
end

piecesButton.MouseButton1Click:Connect(function()
	pieceSet = pieceSet % #PIECE_SETS + 1
	piecesButton.Text = if PIECE_SETS[pieceSet] == "" then "Pieces: plain" else "Pieces: wood"
	draw()
end)

pcall(function()
	board:GetPropertyChangedSignal("AbsoluteSize"):Connect(draw)
end)
draw()
ui.start()
