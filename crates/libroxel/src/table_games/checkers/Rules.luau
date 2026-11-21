--!native
-- Checkers (English draughts) on the dark squares of an 8 by 8 board.
--
-- Squares are numbered 1 to 64: row 0 at the bottom (seat 1's side),
-- column 0 on the left, square = row * 8 + column + 1. Pieces: 0 empty,
-- 1 and 2 are men of seat 1 and 2, 3 and 4 are their kings.
-- Seat 1 plays the dark pieces and moves first, up the board.
--
-- A move is the list of squares the piece stands on: start, then each
-- landing square. Taking is a must, and a piece that can keep jumping must.

local Rules = {}

Rules.NAME = "Checkers"
Rules.TITLE = "Checkers"
Rules.MIN_SEATS = 2
Rules.MAX_SEATS = 2
Rules.LEVELS = { "Easy", "Medium", "Hard" }
Rules.BOT_PAUSE = 0.6
-- Moves in a row with no man moved and nothing taken before it's a draw.
Rules.QUIET_LIMIT = 80

local function rowOf(sq)
	return (sq - 1) // 8
end
local function colOf(sq)
	return (sq - 1) % 8
end
Rules.rowOf, Rules.colOf = rowOf, colOf

local function ownerOf(piece)
	return if piece == 0 then 0 else (piece - 1) % 2 + 1
end
Rules.ownerOf = ownerOf

local function isKing(piece)
	return piece > 2
end
Rules.isKing = isKing

function Rules.new()
	local board = table.create(64, 0)
	for sq = 1, 64 do
		local row, col = rowOf(sq), colOf(sq)
		if (row + col) % 2 == 0 then
			if row <= 2 then
				board[sq] = 1
			elseif row >= 5 then
				board[sq] = 2
			end
		end
	end
	return { board = board, turn = 1, quiet = 0, last = {}, ply = 0 }
end

-- Forward for seat 1 is up (+1 row); for seat 2 down.
local DIRS = { { 1, -1 }, { 1, 1 }, { -1, -1 }, { -1, 1 } }

local function onBoard(row, col)
	return row >= 0 and row < 8 and col >= 0 and col < 8
end

local function canGo(piece, dr)
	if isKing(piece) then
		return true
	end
	return if ownerOf(piece) == 1 then dr == 1 else dr == -1
end

local function lastRow(seat)
	return if seat == 1 then 7 else 0
end

-- Every way the piece on `from` can keep jumping, added to `out`.
local function jumps(board, piece, path, taken, out)
	local sq = path[#path]
	local row, col = rowOf(sq), colOf(sq)
	local seat = ownerOf(piece)
	local found = false
	for _, d in DIRS do
		if canGo(piece, d[1]) then
			local mr, mc = row + d[1], col + d[2]
			local lr, lc = row + 2 * d[1], col + 2 * d[2]
			if onBoard(lr, lc) then
				local mid = mr * 8 + mc + 1
				local land = lr * 8 + lc + 1
				local victim = board[mid]
				if victim ~= 0 and ownerOf(victim) ~= seat and not taken[mid] and board[land] == 0 then
					found = true
					local nextPath = table.clone(path)
					table.insert(nextPath, land)
					local nextTaken = table.clone(taken)
					nextTaken[mid] = true
					if not isKing(piece) and lr == lastRow(seat) then
						-- Crowned: the move ends here.
						table.insert(out, nextPath)
					else
						jumps(board, piece, nextPath, nextTaken, out)
					end
				end
			end
		end
	end
	if not found and #path > 1 then
		table.insert(out, path)
	end
end

-- Every legal move for `seat`: jumps if there are any, else plain steps.
function Rules.moves(state, seat)
	local board = state.board
	local out = {}
	for sq = 1, 64 do
		local piece = board[sq]
		if piece ~= 0 and ownerOf(piece) == seat then
			-- The piece leaves its square while it jumps.
			board[sq] = 0
			jumps(board, piece, { sq }, {}, out)
			board[sq] = piece
		end
	end
	if #out > 0 then
		return out, true
	end
	for sq = 1, 64 do
		local piece = board[sq]
		if piece ~= 0 and ownerOf(piece) == seat then
			local row, col = rowOf(sq), colOf(sq)
			for _, d in DIRS do
				if canGo(piece, d[1]) then
					local r, c = row + d[1], col + d[2]
					if onBoard(r, c) and board[r * 8 + c + 1] == 0 then
						table.insert(out, { sq, r * 8 + c + 1 })
					end
				end
			end
		end
	end
	return out, false
end

local function samePath(a, b)
	if type(a) ~= "table" or #a ~= #b then
		return false
	end
	for i = 1, #b do
		if a[i] ~= b[i] then
			return false
		end
	end
	return true
end

-- Plays a move (already known to be legal) and returns what's needed to
-- take it back.
local function apply(state, path)
	local board = state.board
	local from, to = path[1], path[#path]
	local piece = board[from]
	local undo = { path = path, piece = piece, taken = {}, quiet = state.quiet, turn = state.turn }
	board[from] = 0
	for i = 1, #path - 1 do
		local a, b = path[i], path[i + 1]
		if math.abs(rowOf(a) - rowOf(b)) == 2 then
			local mid = ((rowOf(a) + rowOf(b)) // 2) * 8 + (colOf(a) + colOf(b)) // 2 + 1
			table.insert(undo.taken, { mid, board[mid] })
			board[mid] = 0
		end
	end
	local seat = ownerOf(piece)
	local placed = piece
	if not isKing(piece) and rowOf(to) == lastRow(seat) then
		placed = piece + 2
	end
	board[to] = placed
	if #undo.taken > 0 or not isKing(piece) then
		state.quiet = 0
	else
		state.quiet += 1
	end
	state.turn = 3 - state.turn
	return undo
end

local function unapply(state, undo)
	local board = state.board
	local path = undo.path
	board[path[#path]] = 0
	for _, t in undo.taken do
		board[t[1]] = t[2]
	end
	board[path[1]] = undo.piece
	state.quiet = undo.quiet
	state.turn = undo.turn
end

function Rules.turn(state)
	return state.turn
end

function Rules.play(state, seat, path)
	if state.turn ~= seat then
		return false, "It isn't your turn"
	end
	local moves, mustJump = Rules.moves(state, seat)
	for _, m in moves do
		if samePath(path, m) then
			apply(state, m)
			state.last = m
			state.ply += 1
			return true
		end
	end
	if mustJump then
		return false, "You have to jump when you can"
	end
	return false, "That move isn't allowed"
end

function Rules.result(state)
	local moves = Rules.moves(state, state.turn)
	if #moves == 0 then
		local left = 0
		for _, piece in state.board do
			if ownerOf(piece) == state.turn then
				left += 1
			end
		end
		return { winner = 3 - state.turn, reason = if left == 0 then "Every piece taken" else "No moves left" }
	end
	if state.quiet >= Rules.QUIET_LIMIT then
		return { winner = nil, reason = "40 moves each with nothing taken" }
	end
	return nil
end

function Rules.view(state)
	local moves, mustJump = Rules.moves(state, state.turn)
	return {
		board = table.clone(state.board),
		last = state.last,
		moves = moves,
		mustJump = mustJump,
		ply = state.ply,
	}
end

------------------------------------------------------------------------
-- The computer: negamax with alpha-beta over material, kings, how far men
-- have come and a guarded back row.

local function evaluate(state, seat)
	local board = state.board
	local total = 0
	for sq = 1, 64 do
		local piece = board[sq]
		if piece ~= 0 then
			local owner = ownerOf(piece)
			local row, col = rowOf(sq), colOf(sq)
			local value
			if isKing(piece) then
				value = 175
				-- Kings like the middle.
				value += 6 - math.abs(3.5 - col) - math.abs(3.5 - row)
			else
				local advance = if owner == 1 then row else 7 - row
				value = 100 + advance * 4
				if advance == 0 then
					value += 8
				end
				if col == 0 or col == 7 then
					value -= 3
				end
			end
			total += if owner == seat then value else -value
		end
	end
	return total
end

local WIN = 100000
local nodes = 0

local function search(state, depth, alpha, beta, yield)
	nodes += 1
	if yield and nodes % 1500 == 0 then
		yield()
	end
	local seat = state.turn
	local moves, mustJump = Rules.moves(state, seat)
	if #moves == 0 then
		return -WIN - depth
	end
	if state.quiet >= Rules.QUIET_LIMIT then
		return 0
	end
	-- Keep looking while there are jumps about, so trades are seen through.
	if depth <= 0 and not mustJump then
		return evaluate(state, seat)
	end
	if depth <= -6 then
		return evaluate(state, seat)
	end
	local best = -math.huge
	for _, m in moves do
		local undo = apply(state, m)
		local value = -search(state, depth - 1, -beta, -alpha, yield)
		unapply(state, undo)
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
	return best
end

local DEPTHS = { 1, 4, 7 }
local SLIPS = { 0.4, 0.1, 0 }

function Rules.bot(real, seat, level, yield)
	local state = { board = table.clone(real.board), turn = seat, quiet = real.quiet }
	local moves = Rules.moves(state, seat)
	if #moves == 0 then
		return nil
	end
	if #moves == 1 then
		return moves[1]
	end
	if math.random() < (SLIPS[level] or 0) then
		return moves[math.random(1, #moves)]
	end
	nodes = 0
	local depth = DEPTHS[level] or 4
	local bestValue, best = -math.huge, {}
	for _, m in moves do
		local undo = apply(state, m)
		local value = -search(state, depth - 1, -math.huge, math.huge, yield)
		unapply(state, undo)
		if value > bestValue then
			bestValue, best = value, { m }
		elseif value == bestValue then
			table.insert(best, m)
		end
	end
	return best[math.random(1, #best)]
end

return Rules
