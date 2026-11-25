--!native
-- The rules of chess: positions, legal moves, notation and how games end.
-- Shared by the server (which referees every game and runs the computer
-- players) and each player's screen (which shows the moves a piece has).
--
-- The board is a 10x12 grid numbered from 0 so that a knight's jump off the
-- edge lands on a border square: a1 is 21, h1 28, a8 91 and h8 98.
-- Pieces are numbers: 1 pawn, 2 knight, 3 bishop, 4 rook, 5 queen, 6 king,
-- plus 8 for black. 0 is an empty square and 7 is off the board.
-- A move is one number: from + to * 128 + promotion * 16384 + kind * 131072,
-- where kind is 0 for a plain move, 1 a pawn's double step, 2 en passant
-- and 3 castling.

local Rules = {}

local PAWN, KNIGHT, BISHOP, ROOK, QUEEN, KING = 1, 2, 3, 4, 5, 6
local OFF, EMPTY, BLACK = 7, 0, 8
Rules.PAWN, Rules.KNIGHT, Rules.BISHOP, Rules.ROOK, Rules.QUEEN, Rules.KING = PAWN, KNIGHT, BISHOP, ROOK, QUEEN, KING
Rules.BLACK, Rules.OFF = BLACK, OFF
Rules.START = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"

local KNIGHT_JUMPS = { 21, 19, 12, 8, -8, -12, -19, -21 }
local DIAGONALS = { 11, 9, -9, -11 }
local STRAIGHTS = { 10, -10, 1, -1 }
local AROUND = { 11, 10, 9, 1, -1, -9, -10, -11 }
local LETTERS = { "P", "N", "B", "R", "Q", "K" }

-- The 64 squares of the board, a1 to h8.
local SQUARES = {}
for rank = 0, 7 do
	for file = 0, 7 do
		table.insert(SQUARES, 21 + file + rank * 10)
	end
end
Rules.SQUARES = SQUARES

local function fileOf(sq)
	return (sq - 21) % 10
end
local function rankOf(sq)
	return (sq - 21) // 10
end
Rules.fileOf, Rules.rankOf = fileOf, rankOf

function Rules.square(file, rank)
	return 21 + file + rank * 10
end

function Rules.squareName(sq)
	return string.char(97 + fileOf(sq)) .. tostring(rankOf(sq) + 1)
end

function Rules.parseSquare(text)
	local f, r = string.byte(text, 1), string.byte(text, 2)
	if not f or not r then
		return nil
	end
	f, r = f - 97, r - 49
	if f < 0 or f > 7 or r < 0 or r > 7 then
		return nil
	end
	return 21 + f + r * 10
end

-- Moves as numbers.
local function encode(from, to, promo, kind)
	return from + to * 128 + promo * 16384 + kind * 131072
end
Rules.encode = encode
function Rules.from(m)
	return m % 128
end
function Rules.to(m)
	return (m // 128) % 128
end
function Rules.promotion(m)
	return (m // 16384) % 8
end
function Rules.kind(m)
	return m // 131072
end

-- Castling rights: 1 white short, 2 white long, 4 black short, 8 black long.
-- Moving from or to one of these squares takes rights away.
local KEEP_RIGHTS = table.create(120, 15)
KEEP_RIGHTS[0] = 15
KEEP_RIGHTS[21] = 13
KEEP_RIGHTS[25] = 12
KEEP_RIGHTS[28] = 14
KEEP_RIGHTS[91] = 7
KEEP_RIGHTS[95] = 3
KEEP_RIGHTS[98] = 11

-- Hash numbers for positions: two 32-bit halves, made the same way
-- everywhere so every computer agrees.
local seed = 0x2545F491
local function random32()
	seed = bit32.bxor(seed, bit32.lshift(seed, 13))
	seed = bit32.bxor(seed, bit32.rshift(seed, 17))
	seed = bit32.bxor(seed, bit32.lshift(seed, 5))
	return seed
end
local ZA, ZB = {}, {}
for piece = 0, 15 do
	for sq = 0, 119 do
		ZA[piece * 120 + sq + 1] = random32()
		ZB[piece * 120 + sq + 1] = random32()
	end
end
local SIDE_A, SIDE_B = random32(), random32()
local CASTLE_A, CASTLE_B, EP_A, EP_B = {}, {}, {}, {}
for i = 0, 15 do
	CASTLE_A[i + 1], CASTLE_B[i + 1] = random32(), random32()
end
for f = 0, 7 do
	EP_A[f + 1], EP_B[f + 1] = random32(), random32()
end

local Position = {}
Position.__index = Position
Rules.Position = Position

local function empty()
	local board = table.create(120, OFF)
	board[0] = OFF
	for _, sq in SQUARES do
		board[sq] = EMPTY
	end
	return setmetatable({
		board = board,
		side = 0, -- 0 white to move, 1 black
		castling = 0,
		ep = 0, -- the square a pawn may capture onto en passant, or 0
		halfmove = 0,
		fullmove = 1,
		kings = { [0] = 0, [1] = 0 },
		ha = 0,
		hb = 0,
		-- Undo records, and the hash of each position so far (for repetitions).
		undo = {},
		ply = 0,
		keysA = {},
		keysB = {},
	}, Position)
end

local PIECE_OF = { P = 1, N = 2, B = 3, R = 4, Q = 5, K = 6 }

-- Whether a pawn of the side to move could really take en passant: the
-- square only counts towards repetitions then.
local function epMatters(self)
	if self.ep == 0 then
		return false
	end
	local b, ep = self.board, self.ep
	if self.side == 0 then
		return b[ep - 9] == PAWN or b[ep - 11] == PAWN
	end
	return b[ep + 9] == PAWN + BLACK or b[ep + 11] == PAWN + BLACK
end

function Position:rehash()
	local a, b = 0, 0
	for _, sq in SQUARES do
		local p = self.board[sq]
		if p ~= EMPTY then
			a = bit32.bxor(a, ZA[p * 120 + sq + 1])
			b = bit32.bxor(b, ZB[p * 120 + sq + 1])
		end
	end
	if self.side == 1 then
		a, b = bit32.bxor(a, SIDE_A), bit32.bxor(b, SIDE_B)
	end
	a, b = bit32.bxor(a, CASTLE_A[self.castling + 1]), bit32.bxor(b, CASTLE_B[self.castling + 1])
	if self.ep ~= 0 then
		a, b = bit32.bxor(a, EP_A[fileOf(self.ep) + 1]), bit32.bxor(b, EP_B[fileOf(self.ep) + 1])
	end
	self.ha, self.hb = a, b
end

-- The position's key for repetitions: the hash, with the en passant square
-- left out when no pawn can use it.
function Position:repetitionKey()
	local a, b = self.ha, self.hb
	if self.ep ~= 0 and not epMatters(self) then
		a, b = bit32.bxor(a, EP_A[fileOf(self.ep) + 1]), bit32.bxor(b, EP_B[fileOf(self.ep) + 1])
	end
	return a, b
end

local function record(self)
	local a, b = self:repetitionKey()
	self.keysA[self.ply + 1], self.keysB[self.ply + 1] = a, b
end

-- A position from Forsyth-Edwards Notation, or nil if it can't be read.
function Rules.fromFen(fen)
	local self = empty()
	local parts = string.split(fen or Rules.START, " ")
	local rank, file = 7, 0
	for i = 1, #parts[1] do
		local c = string.sub(parts[1], i, i)
		if c == "/" then
			rank, file = rank - 1, 0
		elseif tonumber(c) then
			file += tonumber(c)
		else
			local kind = PIECE_OF[string.upper(c)]
			if not kind or file > 7 or rank < 0 then
				return nil
			end
			local piece = if c == string.upper(c) then kind else kind + BLACK
			local sq = 21 + file + rank * 10
			self.board[sq] = piece
			if kind == KING then
				self.kings[if piece == KING then 0 else 1] = sq
			end
			file += 1
		end
	end
	if self.kings[0] == 0 or self.kings[1] == 0 then
		return nil
	end
	self.side = if parts[2] == "b" then 1 else 0
	local rights = parts[3] or "-"
	for c, bit in { K = 1, Q = 2, k = 4, q = 8 } do
		if string.find(rights, c, 1, true) then
			self.castling += bit
		end
	end
	self.ep = if parts[4] and parts[4] ~= "-" then Rules.parseSquare(parts[4]) or 0 else 0
	self.halfmove = tonumber(parts[5]) or 0
	self.fullmove = tonumber(parts[6]) or 1
	self:rehash()
	record(self)
	return self
end

function Rules.new()
	return Rules.fromFen(Rules.START)
end

function Position:fen()
	local rows = {}
	for rank = 7, 0, -1 do
		local row, gap = "", 0
		for file = 0, 7 do
			local p = self.board[21 + file + rank * 10]
			if p == EMPTY then
				gap += 1
			else
				if gap > 0 then
					row ..= gap
					gap = 0
				end
				local letter = LETTERS[p % 8]
				row ..= if p >= BLACK then string.lower(letter) else letter
			end
		end
		if gap > 0 then
			row ..= gap
		end
		table.insert(rows, row)
	end
	local rights = ""
	for _, pair in { { 1, "K" }, { 2, "Q" }, { 4, "k" }, { 8, "q" } } do
		if bit32.band(self.castling, pair[1]) ~= 0 then
			rights ..= pair[2]
		end
	end
	return table.concat(rows, "/")
		.. (if self.side == 0 then " w " else " b ")
		.. (if rights == "" then "-" else rights)
		.. " "
		.. (if self.ep ~= 0 then Rules.squareName(self.ep) else "-")
		.. " "
		.. self.halfmove
		.. " "
		.. self.fullmove
end

-- Whether `sq` is attacked by a piece of `by` (0 white, 1 black).
local function attacked(board, sq, by)
	local add = by * BLACK
	if by == 0 then
		if board[sq - 9] == PAWN or board[sq - 11] == PAWN then
			return true
		end
	elseif board[sq + 9] == PAWN + BLACK or board[sq + 11] == PAWN + BLACK then
		return true
	end
	local knight, king = KNIGHT + add, KING + add
	for i = 1, 8 do
		if board[sq + KNIGHT_JUMPS[i]] == knight or board[sq + AROUND[i]] == king then
			return true
		end
	end
	local bishop, rook, queen = BISHOP + add, ROOK + add, QUEEN + add
	for i = 1, 4 do
		local d = DIAGONALS[i]
		local at = sq + d
		local p = board[at]
		while p == EMPTY do
			at += d
			p = board[at]
		end
		if p == bishop or p == queen then
			return true
		end
		d = STRAIGHTS[i]
		at = sq + d
		p = board[at]
		while p == EMPTY do
			at += d
			p = board[at]
		end
		if p == rook or p == queen then
			return true
		end
	end
	return false
end
Rules.attacked = attacked

function Position:inCheck(side)
	side = side or self.side
	return attacked(self.board, self.kings[side], 1 - side)
end

-- Adds the side to move's moves to `list` from index `n` + 1 and returns
-- the new count. They may leave the king in check: `make` checks that.
-- With `capturesOnly`, only captures and promotions.
function Position:generate(list, n, capturesOnly)
	local board, side = self.board, self.side
	local own = side * BLACK
	local forward = if side == 0 then 10 else -10
	local startRank = if side == 0 then 1 else 6
	local lastRank = if side == 0 then 7 else 0
	for _, from in SQUARES do
		local p = board[from]
		if p ~= EMPTY and (p >= BLACK) == (side == 1) then
			local kind = p - own
			if kind == PAWN then
				local to = from + forward
				local promotes = rankOf(to) == lastRank
				if board[to] == EMPTY then
					if promotes then
						n += 1
						list[n] = encode(from, to, QUEEN, 0)
						if not capturesOnly then
							for promo = KNIGHT, ROOK do
								n += 1
								list[n] = encode(from, to, promo, 0)
							end
						end
					elseif not capturesOnly then
						n += 1
						list[n] = encode(from, to, 0, 0)
						if rankOf(from) == startRank and board[to + forward] == EMPTY then
							n += 1
							list[n] = encode(from, to + forward, 0, 1)
						end
					end
				end
				for side_step = -1, 1, 2 do
					local at = to + side_step
					local q = board[at]
					if q ~= OFF and q ~= EMPTY and (q >= BLACK) ~= (side == 1) then
						if promotes then
							for promo = QUEEN, KNIGHT, -1 do
								n += 1
								list[n] = encode(from, at, promo, 0)
							end
						else
							n += 1
							list[n] = encode(from, at, 0, 0)
						end
					elseif at == self.ep and q == EMPTY then
						n += 1
						list[n] = encode(from, at, 0, 2)
					end
				end
			elseif kind == KNIGHT or kind == KING then
				local steps = if kind == KNIGHT then KNIGHT_JUMPS else AROUND
				for i = 1, 8 do
					local to = from + steps[i]
					local q = board[to]
					if q == EMPTY then
						if not capturesOnly then
							n += 1
							list[n] = encode(from, to, 0, 0)
						end
					elseif q ~= OFF and (q >= BLACK) ~= (side == 1) then
						n += 1
						list[n] = encode(from, to, 0, 0)
					end
				end
				if kind == KING and not capturesOnly then
					local rights = self.castling
					local enemy = 1 - side
					if side == 0 and from == 25 then
						if bit32.band(rights, 1) ~= 0 and board[26] == EMPTY and board[27] == EMPTY and board[28] == ROOK
							and not attacked(board, 25, enemy) and not attacked(board, 26, enemy) then
							n += 1
							list[n] = encode(25, 27, 0, 3)
						end
						if bit32.band(rights, 2) ~= 0 and board[24] == EMPTY and board[23] == EMPTY and board[22] == EMPTY
							and board[21] == ROOK and not attacked(board, 25, enemy) and not attacked(board, 24, enemy) then
							n += 1
							list[n] = encode(25, 23, 0, 3)
						end
					elseif side == 1 and from == 95 then
						if bit32.band(rights, 4) ~= 0 and board[96] == EMPTY and board[97] == EMPTY
							and board[98] == ROOK + BLACK and not attacked(board, 95, enemy) and not attacked(board, 96, enemy) then
							n += 1
							list[n] = encode(95, 97, 0, 3)
						end
						if bit32.band(rights, 8) ~= 0 and board[94] == EMPTY and board[93] == EMPTY and board[92] == EMPTY
							and board[91] == ROOK + BLACK and not attacked(board, 95, enemy) and not attacked(board, 94, enemy) then
							n += 1
							list[n] = encode(95, 93, 0, 3)
						end
					end
				end
			else
				local diagonal, straight = kind ~= ROOK, kind ~= BISHOP
				for i = 1, 8 do
					local d = AROUND[i]
					local isDiagonal = d == 11 or d == 9 or d == -9 or d == -11
					if (isDiagonal and diagonal) or (not isDiagonal and straight) then
						local to = from + d
						local q = board[to]
						while q == EMPTY do
							if not capturesOnly then
								n += 1
								list[n] = encode(from, to, 0, 0)
							end
							to += d
							q = board[to]
						end
						if q ~= OFF and (q >= BLACK) ~= (side == 1) then
							n += 1
							list[n] = encode(from, to, 0, 0)
						end
					end
				end
			end
		end
	end
	return n
end

local function toggle(self, piece, sq)
	local i = piece * 120 + sq + 1
	self.ha = bit32.bxor(self.ha, ZA[i])
	self.hb = bit32.bxor(self.hb, ZB[i])
end

-- Plays `m`. Returns false (and leaves the position as it was) if it would
-- leave the mover's king in check.
function Position:make(m)
	local board = self.board
	local from, to = m % 128, (m // 128) % 128
	local promo, kind = (m // 16384) % 8, m // 131072
	local piece = board[from]
	local captured = board[to]
	local side = self.side
	local ply = self.ply + 1
	local u = self.undo[ply]
	if not u then
		u = {}
		self.undo[ply] = u
	end
	u[1], u[2], u[3], u[4], u[5], u[6], u[7] = m, captured, self.castling, self.ep, self.halfmove, self.ha, self.hb
	self.ply = ply

	-- Take the old castling and en passant out of the hash.
	local ha = bit32.bxor(self.ha, CASTLE_A[self.castling + 1])
	local hb = bit32.bxor(self.hb, CASTLE_B[self.castling + 1])
	if self.ep ~= 0 then
		ha, hb = bit32.bxor(ha, EP_A[fileOf(self.ep) + 1]), bit32.bxor(hb, EP_B[fileOf(self.ep) + 1])
	end
	self.ha, self.hb = ha, hb

	if captured ~= EMPTY then
		toggle(self, captured, to)
	end
	toggle(self, piece, from)
	board[from] = EMPTY
	local placed = if promo ~= 0 then promo + side * BLACK else piece
	board[to] = placed
	toggle(self, placed, to)

	if kind == 2 then
		local behind = if side == 0 then to - 10 else to + 10
		u[2] = board[behind]
		toggle(self, board[behind], behind)
		board[behind] = EMPTY
	elseif kind == 3 then
		local rookFrom, rookTo
		if to > from then
			rookFrom, rookTo = from + 3, from + 1
		else
			rookFrom, rookTo = from - 4, from - 1
		end
		local rook = board[rookFrom]
		toggle(self, rook, rookFrom)
		board[rookFrom] = EMPTY
		board[rookTo] = rook
		toggle(self, rook, rookTo)
	end
	if piece % 8 == KING then
		self.kings[side] = to
	end

	self.castling = bit32.band(self.castling, KEEP_RIGHTS[from], KEEP_RIGHTS[to])
	self.ep = if kind == 1 then (from + to) // 2 else 0
	self.ha = bit32.bxor(self.ha, CASTLE_A[self.castling + 1], SIDE_A)
	self.hb = bit32.bxor(self.hb, CASTLE_B[self.castling + 1], SIDE_B)
	if self.ep ~= 0 then
		self.ha = bit32.bxor(self.ha, EP_A[fileOf(self.ep) + 1])
		self.hb = bit32.bxor(self.hb, EP_B[fileOf(self.ep) + 1])
	end
	self.halfmove = if piece % 8 == PAWN or u[2] ~= EMPTY then 0 else self.halfmove + 1
	if side == 1 then
		self.fullmove += 1
	end
	self.side = 1 - side
	if attacked(board, self.kings[side], 1 - side) then
		self:unmake()
		return false
	end
	record(self)
	return true
end

function Position:unmake()
	local ply = self.ply
	local u = self.undo[ply]
	local m, captured = u[1], u[2]
	local board = self.board
	local from, to = m % 128, (m // 128) % 128
	local promo, kind = (m // 16384) % 8, m // 131072
	local side = 1 - self.side
	self.side = side
	local piece = board[to]
	if promo ~= 0 then
		piece = PAWN + side * BLACK
	end
	board[from] = piece
	if kind == 2 then
		board[to] = EMPTY
		board[if side == 0 then to - 10 else to + 10] = captured
	else
		board[to] = captured
	end
	if kind == 3 then
		local rookFrom, rookTo
		if to > from then
			rookFrom, rookTo = from + 3, from + 1
		else
			rookFrom, rookTo = from - 4, from - 1
		end
		board[rookFrom] = board[rookTo]
		board[rookTo] = EMPTY
	end
	if piece % 8 == KING then
		self.kings[side] = from
	end
	self.castling, self.ep, self.halfmove, self.ha, self.hb = u[3], u[4], u[5], u[6], u[7]
	if side == 1 then
		self.fullmove -= 1
	end
	self.ply = ply - 1
end

-- Passes the turn (for the computer's null-move search). Undo with unmakeNull.
function Position:makeNull()
	local ply = self.ply + 1
	local u = self.undo[ply]
	if not u then
		u = {}
		self.undo[ply] = u
	end
	u[1], u[2], u[3], u[4], u[5], u[6], u[7] = 0, EMPTY, self.castling, self.ep, self.halfmove, self.ha, self.hb
	self.ply = ply
	if self.ep ~= 0 then
		self.ha = bit32.bxor(self.ha, EP_A[fileOf(self.ep) + 1])
		self.hb = bit32.bxor(self.hb, EP_B[fileOf(self.ep) + 1])
		self.ep = 0
	end
	self.ha, self.hb = bit32.bxor(self.ha, SIDE_A), bit32.bxor(self.hb, SIDE_B)
	self.side = 1 - self.side
	self.halfmove += 1
	record(self)
end

function Position:unmakeNull()
	local u = self.undo[self.ply]
	self.castling, self.ep, self.halfmove, self.ha, self.hb = u[3], u[4], u[5], u[6], u[7]
	self.side = 1 - self.side
	self.ply -= 1
end

-- How many times the current position has stood before (counting back to
-- the last capture or pawn move, `limit` positions at most).
function Position:repeats(limit)
	local a, b = self.keysA[self.ply + 1], self.keysB[self.ply + 1]
	local count = 0
	local back = math.min(self.halfmove, limit or self.halfmove)
	local i = self.ply + 1 - 2
	while i >= 1 and back >= 2 do
		if self.keysA[i] == a and self.keysB[i] == b then
			count += 1
		end
		i -= 2
		back -= 2
	end
	return count
end

-- Every legal move, as numbers.
function Position:legalMoves()
	local list = {}
	local n = self:generate(list, 0, false)
	local legal = {}
	for i = 1, n do
		if self:make(list[i]) then
			self:unmake()
			table.insert(legal, list[i])
		end
	end
	return legal
end

function Rules.uci(m)
	local promo = (m // 16384) % 8
	local text = Rules.squareName(m % 128) .. Rules.squareName((m // 128) % 128)
	if promo ~= 0 then
		text ..= string.lower(LETTERS[promo])
	end
	return text
end

-- The legal move written `text` in long algebraic form ("e2e4", "e7e8q"),
-- or nil.
function Position:moveFromUci(text)
	if type(text) ~= "string" then
		return nil
	end
	text = string.lower(text)
	for _, m in self:legalMoves() do
		if Rules.uci(m) == text then
			return m
		end
	end
	-- A promotion without a piece named becomes a queen.
	for _, m in self:legalMoves() do
		if Rules.uci(m) == text .. "q" then
			return m
		end
	end
	return nil
end

-- Standard algebraic notation for the legal move `m` here ("Nf3", "exd5",
-- "O-O", "e8=Q+").
function Position:san(m)
	local board = self.board
	local from, to = m % 128, (m // 128) % 128
	local promo, kind = (m // 16384) % 8, m // 131072
	local piece = board[from] % 8
	local text
	if kind == 3 then
		text = if to > from then "O-O" else "O-O-O"
	else
		local capture = board[to] ~= EMPTY or kind == 2
		if piece == PAWN then
			text = if capture then string.char(97 + fileOf(from)) .. "x" else ""
			text ..= Rules.squareName(to)
			if promo ~= 0 then
				text ..= "=" .. LETTERS[promo]
			end
		else
			text = LETTERS[piece]
			local sameFile, sameRank, other = false, false, false
			for _, o in self:legalMoves() do
				local of = o % 128
				if of ~= from and (o // 128) % 128 == to and board[of] % 8 == piece then
					other = true
					if fileOf(of) == fileOf(from) then
						sameFile = true
					end
					if rankOf(of) == rankOf(from) then
						sameRank = true
					end
				end
			end
			if other then
				if not sameFile then
					text ..= string.char(97 + fileOf(from))
				elseif not sameRank then
					text ..= tostring(rankOf(from) + 1)
				else
					text ..= Rules.squareName(from)
				end
			end
			if capture then
				text ..= "x"
			end
			text ..= Rules.squareName(to)
		end
	end
	if self:make(m) then
		if self:inCheck() then
			text ..= if #self:legalMoves() == 0 then "#" else "+"
		end
		self:unmake()
	end
	return text
end

-- The pieces on the board, by kind, for each side.
function Position:material()
	local count = { [0] = { 0, 0, 0, 0, 0, 0 }, [1] = { 0, 0, 0, 0, 0, 0 } }
	local bishopShades = { [0] = {}, [1] = {} }
	for _, sq in SQUARES do
		local p = self.board[sq]
		if p ~= EMPTY then
			local side = if p >= BLACK then 1 else 0
			count[side][p % 8] += 1
			if p % 8 == BISHOP then
				bishopShades[side][(fileOf(sq) + rankOf(sq)) % 2] = true
			end
		end
	end
	return count, bishopShades
end

-- Whether `side` has enough left that it could ever checkmate.
function Position:canMate(side)
	local count = self:material()
	local mine, theirs = count[side], count[1 - side]
	if mine[PAWN] + mine[ROOK] + mine[QUEEN] > 0 then
		return true
	end
	local minors = mine[KNIGHT] + mine[BISHOP]
	if minors >= 2 then
		return true
	end
	-- One minor piece mates only with the other side's own pieces in the way.
	return minors == 1 and (theirs[PAWN] + theirs[KNIGHT] + theirs[BISHOP] + theirs[ROOK]) > 0
end

-- Whether neither side can ever checkmate.
function Position:deadDraw()
	local count, shades = self:material()
	local knights, bishops = 0, 0
	for side = 0, 1 do
		local c = count[side]
		if c[PAWN] + c[ROOK] + c[QUEEN] > 0 then
			return false
		end
		knights += c[KNIGHT]
		bishops += c[BISHOP]
	end
	if knights + bishops <= 1 then
		return true
	end
	-- Only bishops left, all on squares of one colour.
	if knights == 0 then
		local light = shades[0][1] or shades[1][1]
		local dark = shades[0][0] or shades[1][0]
		return not (light and dark)
	end
	return false
end

-- How the game stands: nil while it goes on, or the result ("1-0", "0-1"
-- or "1/2-1/2") and the reason.
function Position:outcome()
	if #self:legalMoves() == 0 then
		if self:inCheck() then
			return if self.side == 0 then "0-1" else "1-0", "checkmate"
		end
		return "1/2-1/2", "stalemate"
	end
	if self:deadDraw() then
		return "1/2-1/2", "insufficient material"
	end
	if self:repeats() >= 2 then
		return "1/2-1/2", "threefold repetition"
	end
	if self.halfmove >= 100 then
		return "1/2-1/2", "fifty-move rule"
	end
	return nil
end

-- The piece on a square as a short name ("wk", "bp"), or nil.
function Position:pieceName(sq)
	local p = self.board[sq]
	if p == EMPTY or p == OFF then
		return nil
	end
	return (if p >= BLACK then "b" else "w") .. string.lower(LETTERS[p % 8])
end

-- Counts the move paths `depth` moves deep (for checking move generation).
function Position:perft(depth)
	if depth == 0 then
		return 1
	end
	local list = {}
	local n = self:generate(list, 0, false)
	local total = 0
	for i = 1, n do
		if self:make(list[i]) then
			total += if depth == 1 then 1 else self:perft(depth - 1)
			self:unmake()
		end
	end
	return total
end

return Rules
