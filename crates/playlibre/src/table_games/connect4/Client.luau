-- The Connect 4 screen, after GNOME Four-in-a-row: a black board with a
-- dashed grid, the "Stars and Rings" marbles and a grey row on top where
-- your marble waits over the column. Click a column to drop it in.

local storage = game:GetService("ReplicatedStorage")
local TweenService = game:GetService("TweenService")

local folder = storage:WaitForChild("TableGame")
local Screen = require(folder:WaitForChild("Screen"))
local Rules = require(folder:WaitForChild("Rules"))
local make = Screen.make

local COLUMNS, ROWS = Rules.COLUMNS, Rules.ROWS
local MARBLE = {
	[1] = "playlibre://c4-red",
	[2] = "playlibre://c4-green",
}
local GRID = Color3.fromRGB(114, 127, 140)
local TOP_ROW = Color3.fromRGB(67, 67, 67)
local INK = Color3.fromRGB(46, 52, 54)

local ui = Screen.new(script.Parent, folder, {
	title = "Connect 4",
	levels = Rules.LEVELS,
	level = 2,
	look = {
		background = Color3.fromRGB(250, 250, 250),
		page = Color3.fromRGB(235, 235, 235),
		ink = INK,
		soft = Color3.fromRGB(120, 124, 126),
		line = Color3.fromRGB(213, 208, 204),
		plain = Color3.fromRGB(222, 221, 218),
		plainInk = INK,
		accent = Color3.fromRGB(53, 132, 228),
		go = Color3.fromRGB(53, 132, 228),
		danger = Color3.fromRGB(224, 27, 36),
		seat = Color3.fromRGB(250, 250, 250),
		turn = Color3.fromRGB(255, 255, 255),
		turnEdge = Color3.fromRGB(53, 132, 228),
		yourTurn = Color3.fromRGB(255, 255, 255),
		resultPage = Color3.fromRGB(255, 255, 255),
		resultEdge = Color3.fromRGB(213, 208, 204),
		waiting = Color3.fromRGB(229, 165, 10),
		toast = Color3.fromRGB(53, 53, 53),
		radius = 6,
	},
	pictureOf = function(seat)
		return MARBLE[seat]
	end,
	seatNote = function(_, seat)
		return if seat == 1 then "Red" else "Green"
	end,
	sounds = {
		move = "playlibre://table/drop",
		bad = "playlibre://table/bad",
		win = "playlibre://table/win",
		lose = "playlibre://table/lose",
		draw = "playlibre://table/lose",
	},
	rules = {
		"Take turns dropping marbles into a column.",
		"Four in a row wins: across, down or diagonal.",
		"Full board is a draw.",
	},
})

-- The board: one row on top for the marble you're about to drop, then the grid.
local SIZE = ROWS + 1
local board = make("Frame", {
	Name = "Board",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.fromScale(0.5, 0.5),
	Size = UDim2.fromScale(1, 1),
	BackgroundColor3 = Color3.fromRGB(0, 0, 0),
	BorderSizePixel = 0,
	ClipsDescendants = true,
}, ui.area)
make("UIAspectRatioConstraint", { AspectRatio = COLUMNS / SIZE }, board)
make("Frame", { Name = "TopRow", Size = UDim2.fromScale(1, 1 / SIZE), BackgroundColor3 = TOP_ROW, BorderSizePixel = 0 }, board)

-- Dashed lines between the cells, and a solid one under the top row.
local DASHES = 70
for i = 1, COLUMNS - 1 do
	for d = 0, DASHES - 1, 2 do
		make("Frame", { Position = UDim2.fromScale(i / COLUMNS, d / DASHES), Size = UDim2.new(0, 1, 1 / DASHES, 0), BackgroundColor3 = GRID, BorderSizePixel = 0, ZIndex = 2 }, board)
	end
end
for i = 2, SIZE - 1 do
	for d = 0, DASHES - 1, 2 do
		make("Frame", { Position = UDim2.fromScale(d / DASHES, i / SIZE), Size = UDim2.new(1 / DASHES, 0, 0, 1), BackgroundColor3 = GRID, BorderSizePixel = 0, ZIndex = 2 }, board)
	end
end
make("Frame", { Position = UDim2.fromScale(0, 1 / SIZE), Size = UDim2.new(1, 0, 0, 1), BackgroundColor3 = GRID, BorderSizePixel = 0, ZIndex = 2 }, board)

local function cellPosition(column, row)
	return UDim2.fromScale((column - 1) / COLUMNS, (SIZE - row) / SIZE)
end

local function marble(parent)
	return make("ImageLabel", {
		Size = UDim2.fromScale(1 / COLUMNS, 1 / SIZE),
		BackgroundTransparency = 1,
		Visible = false,
		ZIndex = 3,
	}, parent)
end

local holes = {}
for row = 1, ROWS do
	for column = 1, COLUMNS do
		local m = marble(board)
		m.Name = "c" .. column .. "r" .. row
		m.Position = cellPosition(column, row)
		holes[(row - 1) * COLUMNS + column] = m
	end
end

-- The marble waiting in the top row.
local preview = marble(board)
preview.Name = "Preview"
preview.ZIndex = 4

local columns = {}
for column = 1, COLUMNS do
	local b = make("TextButton", {
		Name = "Column" .. column,
		Text = "",
		Position = UDim2.fromScale((column - 1) / COLUMNS, 0),
		Size = UDim2.fromScale(1 / COLUMNS, 1),
		BackgroundTransparency = 1,
		AutoButtonColor = false,
		ZIndex = 6,
	}, board)
	columns[column] = b
end

local cells = table.create(COLUMNS * ROWS, 0)
local hover = 0
local blink = true

local function draw()
	local v = ui.view
	local game_ = v and v.game
	local line = {}
	if game_ and game_.line then
		for _, at in game_.line do
			line[(at[2] - 1) * COLUMNS + at[1]] = true
		end
	end
	for i, m in holes do
		local who = cells[i]
		m.Visible = who ~= 0 and (blink or not line[i])
		if who ~= 0 then
			m.Image = MARBLE[who]
		end
	end
	preview.Visible = v ~= nil and v.status == "playing"
	if preview.Visible then
		preview.Image = MARBLE[v.turn]
		preview.Position = cellPosition(if ui.myTurn() and hover > 0 then hover else math.ceil(COLUMNS / 2), SIZE)
	end
end

-- A marble falling into its cell.
local function fall(column, row, who)
	local hole = holes[(row - 1) * COLUMNS + column]
	hole.Visible = false
	local m = marble(board)
	m.Image = MARBLE[who]
	m.Visible = true
	m.ZIndex = 4
	m.Position = cellPosition(column, SIZE)
	local seconds = 0.06 + 0.05 * (SIZE - row)
	TweenService:Create(m, TweenInfo.new(seconds, Enum.EasingStyle.Quad, Enum.EasingDirection.In), { Position = cellPosition(column, row) }):Play()
	task.delay(seconds + 0.02, function()
		m:Destroy()
		draw()
		ui.play("move")
	end)
end

-- The winning four blink, as in Four-in-a-row.
local blinking = 0
local function blinkLine()
	blinking += 1
	local mine = blinking
	task.spawn(function()
		for _ = 1, 8 do
			task.wait(0.25)
			if blinking ~= mine then
				return
			end
			blink = not blink
			draw()
		end
		blink = true
		draw()
	end)
end

ui.onTable = function(v, fresh)
	local before = cells
	cells = table.clone(v.game.cells)
	ui.subText = nil
	if fresh then
		blinking += 1
		blink = true
	end
	draw()
	if not fresh and v.game.last > 0 then
		local i = (v.game.lastRow - 1) * COLUMNS + v.game.last
		if before[i] == 0 and cells[i] ~= 0 then
			fall(v.game.last, v.game.lastRow, cells[i])
		end
	end
	if v.game.line and v.status == "over" and not fresh then
		blinkLine()
	end
end

for column, b in columns do
	b.MouseEnter:Connect(function()
		hover = column
		draw()
	end)
	b.MouseLeave:Connect(function()
		if hover == column then
			hover = 0
			draw()
		end
	end)
	b.MouseButton1Click:Connect(function()
		if not ui.myTurn() then
			return
		end
		if cells[(ROWS - 1) * COLUMNS + column] ~= 0 then
			ui.play("bad")
			return
		end
		ui.send(column)
	end)
end

ui.start()
