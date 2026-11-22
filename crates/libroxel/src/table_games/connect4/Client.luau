-- The Connect 4 screen: a blue grid of holes. Click a column (anywhere in
-- it) to drop your disc in.

local storage = game:GetService("ReplicatedStorage")
local TweenService = game:GetService("TweenService")

local folder = storage:WaitForChild("TableGame")
local Screen = require(folder:WaitForChild("Screen"))
local Rules = require(folder:WaitForChild("Rules"))
local make, round = Screen.make, Screen.round

local COLUMNS, ROWS = Rules.COLUMNS, Rules.ROWS
local DISC = {
	[1] = Color3.fromRGB(225, 50, 45),
	[2] = Color3.fromRGB(250, 205, 40),
}
local HOLE = Color3.fromRGB(235, 240, 248)
local BLUE = Color3.fromRGB(30, 85, 190)

local ui = Screen.new(script.Parent, folder, {
	title = "Connect 4",
	levels = Rules.LEVELS,
	level = 2,
	background = Color3.fromRGB(60, 120, 200),
	colourOf = function(seat)
		return DISC[seat]
	end,
	seatNote = function(_, seat)
		return if seat == 1 then "Red discs" else "Yellow discs"
	end,
	rules = {
		"Take turns dropping discs into a column.",
		"Four in a row wins: across, down or diagonal.",
		"Full board is a draw.",
	},
})

-- The board: a strip for the disc you're about to drop, over the grid.
local board = make("Frame", {
	Name = "Board",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.fromScale(0.5, 0.5),
	Size = UDim2.fromScale(1, 1),
	BackgroundTransparency = 1,
}, ui.area)
make("UIAspectRatioConstraint", { AspectRatio = COLUMNS / (ROWS + 1) }, board)

local grid = make("Frame", {
	Name = "Grid",
	Position = UDim2.fromScale(0, 1 / (ROWS + 1)),
	Size = UDim2.fromScale(1, ROWS / (ROWS + 1)),
	BackgroundColor3 = BLUE,
	ZIndex = 2,
}, board)
round(grid, 18)
make("UIStroke", { Color = Color3.fromRGB(15, 50, 130), Thickness = 4 }, grid)

local function cellPosition(column, row)
	return UDim2.fromScale((column - 0.5) / COLUMNS, 1 - (row - 0.5) / ROWS)
end

local holes = {}
for row = 1, ROWS do
	for column = 1, COLUMNS do
		local hole = make("Frame", {
			Name = "c" .. column .. "r" .. row,
			AnchorPoint = Vector2.new(0.5, 0.5),
			Position = cellPosition(column, row),
			Size = UDim2.fromScale(0.8 / COLUMNS, 0.8 / ROWS),
			BackgroundColor3 = HOLE,
			ZIndex = 3,
		}, grid)
		round(hole, 1000)
		local ring = make("UIStroke", { Color = Color3.fromRGB(255, 255, 255), Thickness = 4, Enabled = false }, hole)
		holes[(row - 1) * COLUMNS + column] = { frame = hole, ring = ring }
	end
end

-- The disc waiting above the column the pointer is over.
local preview = make("Frame", {
	Name = "Preview",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.fromScale(0.5 / COLUMNS, 0.5 / (ROWS + 1)),
	Size = UDim2.fromScale(0.8 / COLUMNS, 0.8 / (ROWS + 1)),
	BackgroundColor3 = DISC[1],
	Visible = false,
	ZIndex = 4,
}, board)
round(preview, 1000)

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

local function draw()
	local v = ui.view
	local game_ = v and v.game
	for i, hole in holes do
		local who = cells[i]
		hole.frame.BackgroundColor3 = if who == 0 then HOLE else DISC[who]
		hole.ring.Enabled = false
	end
	if game_ and game_.line then
		for _, at in game_.line do
			holes[(at[2] - 1) * COLUMNS + at[1]].ring.Enabled = true
		end
	end
	local canDrop = ui.myTurn() and hover > 0 and cells[(ROWS - 1) * COLUMNS + hover] == 0
	preview.Visible = canDrop
	if canDrop then
		preview.BackgroundColor3 = DISC[v.you]
		preview.Position = UDim2.fromScale((hover - 0.5) / COLUMNS, 0.5 / (ROWS + 1))
	end
end

-- A disc falling into its hole.
local function fall(column, row, who)
	local hole = holes[(row - 1) * COLUMNS + column]
	hole.frame.BackgroundColor3 = HOLE
	local disc = make("Frame", {
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale((column - 0.5) / COLUMNS, 0.5 / (ROWS + 1)),
		Size = UDim2.fromScale(0.8 / COLUMNS, 0.8 / (ROWS + 1)),
		BackgroundColor3 = DISC[who],
		ZIndex = 4,
	}, board)
	round(disc, 1000)
	local target = UDim2.fromScale((column - 0.5) / COLUMNS, (ROWS - row + 1.5) / (ROWS + 1))
	local seconds = 0.08 + 0.04 * (ROWS - row + 1)
	TweenService:Create(disc, TweenInfo.new(seconds, Enum.EasingStyle.Quad, Enum.EasingDirection.In), { Position = target }):Play()
	task.delay(seconds + 0.02, function()
		disc:Destroy()
		if cells[(row - 1) * COLUMNS + column] == who then
			hole.frame.BackgroundColor3 = DISC[who]
		end
		ui.sounds.move:Play()
	end)
end

ui.onTable = function(v, fresh)
	local before = cells
	cells = table.clone(v.game.cells)
	ui.subText = nil
	draw()
	if not fresh and v.game.last > 0 then
		local i = (v.game.lastRow - 1) * COLUMNS + v.game.last
		if before[i] == 0 and cells[i] ~= 0 then
			fall(v.game.last, v.game.lastRow, cells[i])
		end
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
			ui.toast("That column is full")
			return
		end
		ui.send(column)
	end)
end

ui.start()
