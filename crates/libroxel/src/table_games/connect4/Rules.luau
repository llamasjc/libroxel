--!native
-- Connect 4: drop discs into a 7 by 6 grid and get four in a row across,
-- up or diagonally. The board is a list of 42 cells, row 1 at the bottom:
-- cell (column, row) is (row - 1) * 7 + column, holding 0 (empty), 1 or 2.

local Rules = {}

Rules.NAME = "ConnectFour"
Rules.TITLE = "Connect 4"
Rules.MIN_SEATS = 2
Rules.MAX_SEATS = 2
Rules.LEVELS = { "Easy", "Medium", "Hard" }
Rules.BOT_PAUSE = 0.6
Rules.COLUMNS = 7
Rules.ROWS = 6

local COLUMNS, ROWS = 7, 6

function Rules.new()
	local cells = table.create(COLUMNS * ROWS, 0)
	return { cells = cells, heights = table.create(COLUMNS, 0), turn = 1, count = 0, last = 0, line = nil }
end

local function at(state, column, row)
	if column < 1 or column > COLUMNS or row < 1 or row > ROWS then
		return -1
	end
	return state.cells[(row - 1) * COLUMNS + column]
end

-- The four in a row through (column, row), if there is one.
local DIRECTIONS = { { 1, 0 }, { 0, 1 }, { 1, 1 }, { 1, -1 } }
local function lineThrough(state, column, row)
	local who = at(state, column, row)
	if who <= 0 then
		return nil
	end
	for _, d in DIRECTIONS do
		local line = { { column, row } }
		for _, sign in { 1, -1 } do
			local c, r = column + d[1] * sign, row + d[2] * sign
			while at(state, c, r) == who do
				table.insert(line, { c, r })
				c, r = c + d[1] * sign, r + d[2] * sign
			end
		end
		if #line >= 4 then
			return line
		end
	end
	return nil
end

local function drop(state, column)
	local row = state.heights[column] + 1
	state.cells[(row - 1) * COLUMNS + column] = state.turn
	state.heights[column] = row
	state.count += 1
	return row
end

local function undrop(state, column)
	local row = state.heights[column]
	state.cells[(row - 1) * COLUMNS + column] = 0
	state.heights[column] = row - 1
	state.count -= 1
end

function Rules.turn(state)
	return state.turn
end

function Rules.legal(state, column)
	return type(column) == "number" and column == math.floor(column) and column >= 1 and column <= COLUMNS
		and state.heights[column] < ROWS
end

function Rules.play(state, seat, column)
	if state.line or state.turn ~= seat then
		return false, "It isn't your turn"
	end
	if not Rules.legal(state, column) then
		return false, "That column is full"
	end
	local row = drop(state, column)
	state.last = column
	state.line = lineThrough(state, column, row)
	state.turn = 3 - state.turn
	return true
end

function Rules.result(state)
	if state.line then
		local first = state.line[1]
		return { winner = at(state, first[1], first[2]), reason = "Four in a row" }
	end
	if state.count >= COLUMNS * ROWS then
		return { winner = nil, reason = "The board is full" }
	end
	return nil
end

function Rules.view(state)
	return {
		cells = table.clone(state.cells),
		last = state.last,
		lastRow = if state.last > 0 then state.heights[state.last] else 0,
		line = state.line or false,
	}
end

------------------------------------------------------------------------
-- The computer: a negamax search with alpha-beta, middle columns first,
-- that scores every window of four by how full it is.

local WINDOWS = {}
for row = 1, ROWS do
	for column = 1, COLUMNS do
		for _, d in DIRECTIONS do
			local endC, endR = column + d[1] * 3, row + d[2] * 3
			if endC >= 1 and endC <= COLUMNS and endR >= 1 and endR <= ROWS then
				local window = {}
				for k = 0, 3 do
					table.insert(window, (row + d[2] * k - 1) * COLUMNS + column + d[1] * k)
				end
				table.insert(WINDOWS, window)
			end
		end
	end
end
local ORDER = { 4, 3, 5, 2, 6, 1, 7 }

local function score(state, who)
	local cells = state.cells
	local total = 0
	for _, window in WINDOWS do
		local mine, theirs = 0, 0
		for _, i in window do
			local v = cells[i]
			if v == who then
				mine += 1
			elseif v ~= 0 then
				theirs += 1
			end
		end
		if theirs == 0 then
			total += if mine == 3 then 50 elseif mine == 2 then 8 elseif mine == 1 then 1 else 0
		elseif mine == 0 then
			total -= if theirs == 3 then 50 elseif theirs == 2 then 8 elseif theirs == 1 then 1 else 0
		end
	end
	for row = 1, ROWS do
		local v = cells[(row - 1) * COLUMNS + 4]
		if v == who then
			total += 4
		elseif v ~= 0 then
			total -= 4
		end
	end
	return total
end

local WIN = 100000
local nodes = 0

local function search(state, depth, alpha, beta, yield)
	nodes += 1
	if yield and nodes % 2000 == 0 then
		yield()
	end
	if state.count >= COLUMNS * ROWS then
		return 0
	end
	local who = state.turn
	-- Win at once if we can.
	for _, column in ORDER do
		if state.heights[column] < ROWS then
			local row = drop(state, column)
			local won = lineThrough(state, column, row) ~= nil
			undrop(state, column)
			if won then
				return WIN + depth
			end
		end
	end
	if depth == 0 then
		return score(state, who)
	end
	local best = -math.huge
	for _, column in ORDER do
		if state.heights[column] < ROWS then
			drop(state, column)
			state.turn = 3 - who
			local value = -search(state, depth - 1, -beta, -alpha, yield)
			state.turn = who
			undrop(state, column)
			if value > best then
				best = value
			end
			if best > alpha then
				alpha = best
			end
			if alpha >= beta then
				break
			end
		end
	end
	return best
end

local DEPTHS = { 1, 4, 7 }
local SLIPS = { 0.35, 0.08, 0 }

function Rules.bot(real, seat, level, yield)
	local state = { cells = table.clone(real.cells), heights = table.clone(real.heights), turn = seat, count = real.count }
	local moves = {}
	for _, column in ORDER do
		if state.heights[column] < ROWS then
			table.insert(moves, column)
		end
	end
	if #moves == 0 then
		return nil
	end
	if math.random() < (SLIPS[level] or 0) then
		return moves[math.random(1, #moves)]
	end
	nodes = 0
	local depth = DEPTHS[level] or 4
	local bestValue, best = -math.huge, {}
	for _, column in moves do
		drop(state, column)
		local row = state.heights[column]
		local value
		if lineThrough(state, column, row) then
			value = WIN * 2
		else
			state.turn = 3 - seat
			value = -search(state, depth - 1, -math.huge, math.huge, yield)
			state.turn = seat
		end
		undrop(state, column)
		if value > bestValue then
			bestValue, best = value, { column }
		elseif value == bestValue then
			table.insert(best, column)
		end
	end
	return best[math.random(1, #best)]
end

return Rules
