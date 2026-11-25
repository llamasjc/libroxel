--!native
-- The computer player: an alpha-beta search with a transposition table,
-- quiescence, null moves, late move reductions, killer moves and history,
-- over an evaluation of material, piece squares, pawns and king safety.
-- It thinks in short slices and yields between them, so the game (and
-- everyone else's games) keep running while it thinks.

local Rules = require(script.Parent:WaitForChild("Rules"))

local Engine = {}

local PAWN, KNIGHT, BISHOP, ROOK, QUEEN, KING = 1, 2, 3, 4, 5, 6
local BLACK, EMPTY = 8, 0
local SQUARES = Rules.SQUARES
local fileOf, rankOf = Rules.fileOf, Rules.rankOf

local MATE = 100000
local INFINITE = 1000000
Engine.MATE = MATE

-- The computer players, weakest first. `depth` caps how far each looks,
-- `noise` (in hundredths of a pawn) blurs how it judges moves and `slip`
-- is the chance it plays a move it hasn't looked at properly.
Engine.BOTS = {
	{ name = "Droplet", rating = 250, depth = 1, noise = 320, slip = 0.3, time = 0.4, colour = Color3.fromRGB(120, 200, 255), about = "Just learned how the pieces move." },
	{ name = "Breeze", rating = 500, depth = 1, noise = 160, slip = 0.15, time = 0.5, colour = Color3.fromRGB(130, 220, 200), about = "Likes to attack, forgets to defend." },
	{ name = "Pebble", rating = 800, depth = 2, noise = 90, slip = 0.07, time = 0.6, colour = Color3.fromRGB(170, 190, 150), about = "Steady, but misses tactics." },
	{ name = "Coral", rating = 1100, depth = 3, noise = 50, slip = 0.03, time = 0.8, colour = Color3.fromRGB(255, 150, 130), about = "Spots a hanging piece." },
	{ name = "Aurora", rating = 1400, depth = 4, noise = 25, slip = 0, time = 1.2, colour = Color3.fromRGB(170, 140, 255), about = "Plays solid club chess." },
	{ name = "Glacier", rating = 1700, depth = 6, noise = 10, slip = 0, time = 2, colour = Color3.fromRGB(150, 210, 240), about = "Patient and hard to crack." },
	{ name = "Nimbus", rating = 2000, depth = 64, noise = 0, slip = 0, time = 3, colour = Color3.fromRGB(110, 160, 255), about = "Calculates deep and punishes mistakes." },
	{ name = "Zenith", rating = 2300, depth = 64, noise = 0, slip = 0, time = 6, colour = Color3.fromRGB(255, 205, 90), about = "Full strength. Good luck." },
}

-- A few opening moves, so games don't all start the same way.
local BOOK = {
	[""] = { "e2e4", "d2d4", "c2c4", "g1f3", "e2e4", "d2d4" },
	["e2e4"] = { "e7e5", "c7c5", "e7e6", "c7c6", "e7e5", "d7d5" },
	["d2d4"] = { "d7d5", "g8f6", "g8f6", "e7e6" },
	["c2c4"] = { "e7e5", "g8f6", "c7c5" },
	["g1f3"] = { "d7d5", "g8f6", "c7c5" },
	["e2e4 e7e5"] = { "g1f3", "g1f3", "f1c4", "b1c3" },
	["e2e4 c7c5"] = { "g1f3", "b1c3", "c2c3" },
	["e2e4 e7e6"] = { "d2d4" },
	["e2e4 c7c6"] = { "d2d4" },
	["e2e4 d7d5"] = { "e4d5" },
	["d2d4 d7d5"] = { "c2c4", "g1f3", "c1f4" },
	["d2d4 g8f6"] = { "c2c4", "g1f3" },
	["e2e4 e7e5 g1f3"] = { "b8c6", "b8c6", "g8f6" },
	["e2e4 e7e5 g1f3 b8c6"] = { "f1b5", "f1c4", "d2d4" },
	["d2d4 d7d5 c2c4"] = { "e7e6", "c7c6", "d5c4" },
	["d2d4 g8f6 c2c4"] = { "e7e6", "g7g6" },
}

-- Material and where each piece likes to stand, for the middle game and
-- the end game, from white's side with a8 first.
local VALUE_MG = { 90, 320, 335, 480, 950, 0 }
local VALUE_EG = { 110, 300, 320, 520, 950, 0 }
local PHASE = { 0, 1, 1, 2, 4, 0 }

local TABLES_MG = {
	{ -- pawn
		0, 0, 0, 0, 0, 0, 0, 0,
		60, 60, 60, 60, 60, 60, 60, 60,
		15, 15, 25, 35, 35, 25, 15, 15,
		5, 5, 12, 28, 28, 12, 5, 5,
		0, 0, 8, 22, 22, 0, 0, 0,
		5, -2, -6, 4, 4, -10, -2, 5,
		5, 10, 10, -18, -18, 10, 10, 5,
		0, 0, 0, 0, 0, 0, 0, 0,
	},
	{ -- knight
		-50, -38, -30, -30, -30, -30, -38, -50,
		-38, -18, 0, 2, 2, 0, -18, -38,
		-30, 2, 12, 16, 16, 12, 2, -30,
		-30, 6, 16, 22, 22, 16, 6, -30,
		-30, 2, 16, 22, 22, 16, 2, -30,
		-30, 6, 12, 16, 16, 12, 6, -30,
		-38, -18, 0, 6, 6, 0, -18, -38,
		-50, -30, -30, -30, -30, -30, -30, -50,
	},
	{ -- bishop
		-20, -10, -10, -10, -10, -10, -10, -20,
		-10, 0, 0, 0, 0, 0, 0, -10,
		-10, 0, 6, 10, 10, 6, 0, -10,
		-10, 6, 6, 10, 10, 6, 6, -10,
		-10, 0, 12, 10, 10, 12, 0, -10,
		-10, 10, 10, 10, 10, 10, 10, -10,
		-10, 8, 0, 0, 0, 0, 8, -10,
		-20, -10, -14, -10, -10, -14, -10, -20,
	},
	{ -- rook
		0, 0, 0, 0, 0, 0, 0, 0,
		10, 14, 14, 14, 14, 14, 14, 10,
		-5, 0, 0, 0, 0, 0, 0, -5,
		-5, 0, 0, 0, 0, 0, 0, -5,
		-5, 0, 0, 0, 0, 0, 0, -5,
		-5, 0, 0, 0, 0, 0, 0, -5,
		-5, 0, 0, 0, 0, 0, 0, -5,
		-4, 0, 3, 8, 8, 6, 0, -4,
	},
	{ -- queen
		-20, -10, -10, -5, -5, -10, -10, -20,
		-10, 0, 0, 0, 0, 0, 0, -10,
		-10, 0, 5, 5, 5, 5, 0, -10,
		-5, 0, 5, 5, 5, 5, 0, -5,
		0, 0, 5, 5, 5, 5, 0, -5,
		-10, 5, 5, 5, 5, 5, 0, -10,
		-10, 0, 5, 0, 0, 0, 0, -10,
		-20, -10, -10, -2, -5, -10, -10, -20,
	},
	{ -- king
		-30, -40, -40, -50, -50, -40, -40, -30,
		-30, -40, -40, -50, -50, -40, -40, -30,
		-30, -40, -40, -50, -50, -40, -40, -30,
		-30, -40, -40, -50, -50, -40, -40, -30,
		-20, -30, -30, -40, -40, -30, -30, -20,
		-10, -20, -20, -24, -24, -20, -20, -10,
		12, 12, -6, -10, -10, -6, 14, 14,
		16, 26, 12, -4, 0, 6, 30, 18,
	},
}
local TABLES_EG = {
	{ -- pawn
		0, 0, 0, 0, 0, 0, 0, 0,
		110, 105, 100, 95, 95, 100, 105, 110,
		60, 58, 52, 48, 48, 52, 58, 60,
		30, 28, 22, 18, 18, 22, 28, 30,
		14, 12, 8, 6, 6, 8, 12, 14,
		4, 4, 2, 2, 2, 2, 4, 4,
		0, 0, 0, 0, 0, 0, 0, 0,
		0, 0, 0, 0, 0, 0, 0, 0,
	},
	TABLES_MG[2],
	{
		-14, -10, -8, -6, -6, -8, -10, -14,
		-10, -2, 0, 2, 2, 0, -2, -10,
		-8, 0, 6, 8, 8, 6, 0, -8,
		-6, 2, 8, 12, 12, 8, 2, -6,
		-6, 2, 8, 12, 12, 8, 2, -6,
		-8, 0, 6, 8, 8, 6, 0, -8,
		-10, -2, 0, 2, 2, 0, -2, -10,
		-14, -10, -8, -6, -6, -8, -10, -14,
	},
	{
		6, 6, 6, 6, 6, 6, 6, 6,
		8, 8, 8, 8, 8, 8, 8, 8,
		2, 2, 2, 2, 2, 2, 2, 2,
		0, 0, 0, 0, 0, 0, 0, 0,
		0, 0, 0, 0, 0, 0, 0, 0,
		-2, -2, -2, -2, -2, -2, -2, -2,
		-4, -4, -4, -4, -4, -4, -4, -4,
		-6, -4, -2, 0, 0, -2, -4, -6,
	},
	{
		-10, -6, -4, 0, 0, -4, -6, -10,
		-6, 0, 6, 10, 10, 6, 0, -6,
		-4, 6, 12, 16, 16, 12, 6, -4,
		0, 10, 16, 22, 22, 16, 10, 0,
		0, 10, 16, 22, 22, 16, 10, 0,
		-4, 6, 12, 16, 16, 12, 6, -4,
		-6, 0, 6, 10, 10, 6, 0, -6,
		-10, -6, -4, 0, 0, -4, -6, -10,
	},
	{
		-50, -36, -26, -18, -18, -26, -36, -50,
		-30, -12, 0, 6, 6, 0, -12, -30,
		-26, 2, 18, 26, 26, 18, 2, -26,
		-22, 6, 26, 34, 34, 26, 6, -22,
		-22, 6, 26, 34, 34, 26, 6, -22,
		-26, 2, 18, 26, 26, 18, 2, -26,
		-32, -14, 0, 4, 4, 0, -14, -32,
		-54, -38, -28, -22, -22, -28, -38, -54,
	},
}

-- Piece-square values by board square, with the piece's value added: MG[piece][sq].
local MG, EG = {}, {}
for piece = 1, 14 do
	MG[piece], EG[piece] = table.create(120, 0), table.create(120, 0)
end
for kind = 1, 6 do
	for _, sq in SQUARES do
		local f, r = fileOf(sq), rankOf(sq)
		local white = (7 - r) * 8 + f + 1
		local black = r * 8 + f + 1
		MG[kind][sq] = VALUE_MG[kind] + TABLES_MG[kind][white]
		EG[kind][sq] = VALUE_EG[kind] + TABLES_EG[kind][white]
		MG[kind + BLACK][sq] = VALUE_MG[kind] + TABLES_MG[kind][black]
		EG[kind + BLACK][sq] = VALUE_EG[kind] + TABLES_EG[kind][black]
	end
end

local PASSED_BONUS = { 0, 8, 14, 26, 46, 76, 120, 0 } -- by how far the pawn has come
local pawnFiles = { [0] = table.create(10, 0), [1] = table.create(10, 0) }
local pawnLead = { [0] = table.create(10, -1), [1] = table.create(10, 9) }

-- How good the position is for the side to move, in hundredths of a pawn.
local function evaluate(pos)
	local board = pos.board
	local mg, eg, phase = 0, 0, 0
	local wf, bf = pawnFiles[0], pawnFiles[1]
	-- The most advanced white pawn and the least advanced black one on each file
	-- (files are 1-8 here, with 0 and 9 empty either side).
	local wBack, bBack = pawnLead[0], pawnLead[1]
	for f = 0, 9 do
		wf[f + 1], bf[f + 1] = 0, 0
		wBack[f + 1], bBack[f + 1] = 9, -1
	end
	local bishops0, bishops1 = 0, 0
	for i = 1, 64 do
		local sq = SQUARES[i]
		local p = board[sq]
		if p ~= EMPTY then
			local kind = p % 8
			if p < BLACK then
				mg += MG[p][sq]
				eg += EG[p][sq]
				if kind == PAWN then
					local f = fileOf(sq) + 2
					wf[f] += 1
					local r = rankOf(sq)
					if r < wBack[f] then
						wBack[f] = r
					end
				elseif kind == BISHOP then
					bishops0 += 1
				end
			else
				mg -= MG[p][sq]
				eg -= EG[p][sq]
				if kind == PAWN then
					local f = fileOf(sq) + 2
					bf[f] += 1
					local r = rankOf(sq)
					if r > bBack[f] then
						bBack[f] = r
					end
				elseif kind == BISHOP then
					bishops1 += 1
				end
			end
			phase += PHASE[kind]
		end
	end
	if bishops0 >= 2 then
		mg += 30
		eg += 45
	end
	if bishops1 >= 2 then
		mg -= 30
		eg -= 45
	end
	-- Pawns: doubled and isolated ones are weak, passed ones strong.
	for i = 1, 64 do
		local sq = SQUARES[i]
		local p = board[sq]
		if p == PAWN or p == PAWN + BLACK then
			local f = fileOf(sq) + 2
			local r = rankOf(sq)
			if p == PAWN then
				if wf[f - 1] == 0 and wf[f + 1] == 0 then
					mg -= 12
					eg -= 16
				end
				-- Passed: no black pawn ahead on this file or the next ones.
				if bBack[f] <= r and bBack[f - 1] <= r and bBack[f + 1] <= r then
					mg += PASSED_BONUS[r + 1] // 2
					eg += PASSED_BONUS[r + 1]
				end
			else
				if bf[f - 1] == 0 and bf[f + 1] == 0 then
					mg += 12
					eg += 16
				end
				local fromTop = 7 - r
				if wBack[f] >= r and wBack[f - 1] >= r and wBack[f + 1] >= r then
					mg -= PASSED_BONUS[fromTop + 1] // 2
					eg -= PASSED_BONUS[fromTop + 1]
				end
			end
		end
	end
	for f = 2, 9 do
		if wf[f] > 1 then
			mg -= 10 * (wf[f] - 1)
			eg -= 20 * (wf[f] - 1)
		end
		if bf[f] > 1 then
			mg += 10 * (bf[f] - 1)
			eg += 20 * (bf[f] - 1)
		end
	end
	-- Rooks like open files.
	for i = 1, 64 do
		local sq = SQUARES[i]
		local p = board[sq]
		if p == ROOK then
			local f = fileOf(sq) + 2
			if wf[f] == 0 then
				mg += if bf[f] == 0 then 22 else 10
			end
		elseif p == ROOK + BLACK then
			local f = fileOf(sq) + 2
			if bf[f] == 0 then
				mg -= if wf[f] == 0 then 22 else 10
			end
		end
	end
	-- King shelter: pawns in front of a castled king.
	local k0, k1 = pos.kings[0], pos.kings[1]
	local shelter = 0
	for d = 9, 11 do
		if board[k0 + d] == PAWN then
			shelter += 8
		end
		if board[k1 - d] == PAWN + BLACK then
			shelter -= 8
		end
	end
	mg += shelter

	-- The end game: a winning side drives the other king to the edge.
	if phase < 8 then
		local lead = eg
		if math.abs(lead) > 250 then
			local loser = if lead > 0 then k1 else k0
			local lf, lr = fileOf(loser), rankOf(loser)
			local edge = math.max(3 - lf, lf - 4) + math.max(3 - lr, lr - 4)
			local winner = if lead > 0 then k0 else k1
			local apart = math.abs(fileOf(winner) - lf) + math.abs(rankOf(winner) - lr)
			local push = edge * 12 + (14 - apart) * 5
			eg += if lead > 0 then push else -push
		end
	end
	if phase > 24 then
		phase = 24
	end
	local score = (mg * phase + eg * (24 - phase)) // 24
	-- Tempo.
	score += if pos.side == 0 then 10 else -10
	return if pos.side == 0 then score else -score
end
Engine.evaluate = evaluate

-- A searcher keeps its tables between moves.
local Searcher = {}
Searcher.__index = Searcher

local TT_SIZE = 2 ^ 17
local EXACT, LOWER, UPPER = 0, 1, 2

function Engine.new()
	return setmetatable({
		ttA = table.create(TT_SIZE, -1),
		ttB = table.create(TT_SIZE, 0),
		ttMove = table.create(TT_SIZE, 0),
		ttDepth = table.create(TT_SIZE, 0),
		ttScore = table.create(TT_SIZE, 0),
		ttFlag = table.create(TT_SIZE, 0),
		killers1 = table.create(128, 0),
		killers2 = table.create(128, 0),
		history = table.create(16 * 128, 0),
		moveLists = {},
		scoreLists = {},
		nodes = 0,
	}, Searcher)
end

local function toTT(score, ply)
	if score > MATE - 1000 then
		return score + ply
	elseif score < -MATE + 1000 then
		return score - ply
	end
	return score
end
local function fromTT(score, ply)
	if score > MATE - 1000 then
		return score - ply
	elseif score < -MATE + 1000 then
		return score + ply
	end
	return score
end

function Searcher:tick()
	local now = os.clock()
	if now >= self.deadline then
		self.stopped = true
		return
	end
	if self.yield and now - self.sliceStart > 0.012 then
		self.yield()
		self.sliceStart = os.clock()
	end
end

local VICTIM = { 1, 3, 3, 5, 9, 0 }

function Searcher:order(pos, list, scores, n, ttMove, ply)
	local board = pos.board
	local k1, k2 = self.killers1[ply + 1], self.killers2[ply + 1]
	local history = self.history
	for i = 1, n do
		local m = list[i]
		local s
		if m == ttMove then
			s = 10000000
		else
			local to = (m // 128) % 128
			local victim = board[to]
			local promo = (m // 16384) % 8
			if victim ~= EMPTY then
				s = 1000000 + VICTIM[victim % 8] * 100 - VICTIM[board[m % 128] % 8]
			elseif m // 131072 == 2 then
				s = 1000099
			elseif promo == QUEEN then
				s = 900000
			elseif m == k1 then
				s = 800000
			elseif m == k2 then
				s = 790000
			else
				s = history[board[m % 128] * 128 + to + 1]
				if promo ~= 0 then
					s -= 100000
				end
			end
		end
		scores[i] = s
	end
end

-- Moves the best-scored move from i..n to i.
local function pick(list, scores, i, n)
	local best, at = scores[i], i
	for j = i + 1, n do
		if scores[j] > best then
			best, at = scores[j], j
		end
	end
	if at ~= i then
		list[i], list[at] = list[at], list[i]
		scores[i], scores[at] = scores[at], scores[i]
	end
	return list[i]
end

function Searcher:lists(ply)
	local list = self.moveLists[ply + 1]
	if not list then
		list = {}
		self.moveLists[ply + 1] = list
		self.scoreLists[ply + 1] = {}
	end
	return list, self.scoreLists[ply + 1]
end

function Searcher:quiesce(pos, alpha, beta, ply)
	self.nodes += 1
	if self.nodes % 1024 == 0 then
		self:tick()
	end
	if self.stopped then
		return 0
	end
	local standing = evaluate(pos)
	if standing >= beta then
		return standing
	end
	if standing > alpha then
		alpha = standing
	end
	if ply >= 120 then
		return standing
	end
	local list, scores = self:lists(ply)
	local n = pos:generate(list, 0, true)
	self:order(pos, list, scores, n, 0, ply)
	local board = pos.board
	for i = 1, n do
		local m = pick(list, scores, i, n)
		-- Delta pruning: even winning this piece can't lift the score to alpha.
		local victim = board[(m // 128) % 128]
		local gain = if victim ~= EMPTY then VALUE_EG[victim % 8] else 100
		if (m // 16384) % 8 ~= 0 then
			gain += 850
		end
		if standing + gain + 200 >= alpha and pos:make(m) then
			local score = -self:quiesce(pos, -beta, -alpha, ply + 1)
			pos:unmake()
			if self.stopped then
				return 0
			end
			if score >= beta then
				return score
			end
			if score > alpha then
				alpha = score
			end
		end
	end
	return alpha
end

local function hasPieces(pos, side)
	local board = pos.board
	local lo = side * BLACK
	for i = 1, 64 do
		local p = board[SQUARES[i]]
		if p > lo + PAWN and p < lo + KING then
			return true
		end
	end
	return false
end

function Searcher:search(pos, depth, alpha, beta, ply, allowNull)
	if ply > 0 then
		if pos.halfmove >= 100 or pos:repeats() >= 1 then
			return 0
		end
		-- No mate can be faster than one already found.
		local worst, best = -MATE + ply, MATE - ply - 1
		if alpha < worst then
			alpha = worst
		end
		if beta > best then
			beta = best
		end
		if alpha >= beta then
			return alpha
		end
	end
	local inCheck = pos:inCheck()
	if inCheck then
		depth += 1
	end
	if depth <= 0 then
		return self:quiesce(pos, alpha, beta, ply)
	end
	self.nodes += 1
	if self.nodes % 1024 == 0 then
		self:tick()
	end
	if self.stopped then
		return 0
	end
	if ply >= 100 then
		return evaluate(pos)
	end

	local slot = pos.ha % TT_SIZE + 1
	local ttMove = 0
	local pvNode = beta - alpha > 1
	if self.ttA[slot] == pos.ha and self.ttB[slot] == pos.hb then
		ttMove = self.ttMove[slot]
		if ply > 0 and not pvNode and self.ttDepth[slot] >= depth then
			local score = fromTT(self.ttScore[slot], ply)
			local flag = self.ttFlag[slot]
			if flag == EXACT or (flag == LOWER and score >= beta) or (flag == UPPER and score <= alpha) then
				return score
			end
		end
	end

	local static = if inCheck then -INFINITE else evaluate(pos)
	if not pvNode and not inCheck then
		-- Reverse futility: far enough ahead that a shallow search won't change it.
		if depth <= 3 and static - 120 * depth >= beta and math.abs(beta) < MATE - 1000 then
			return static
		end
		-- Null move: even passing keeps us above beta.
		if allowNull and depth >= 3 and static >= beta and hasPieces(pos, pos.side) then
			pos:makeNull()
			local r = if depth >= 6 then 3 else 2
			local score = -self:search(pos, depth - 1 - r, -beta, -beta + 1, ply + 1, false)
			pos:unmakeNull()
			if self.stopped then
				return 0
			end
			if score >= beta then
				return if score > MATE - 1000 then beta else score
			end
		end
	end

	local list, scores = self:lists(ply)
	local n = pos:generate(list, 0, false)
	self:order(pos, list, scores, n, ttMove, ply)
	local board = pos.board
	local bestScore, bestMove = -INFINITE, 0
	local startAlpha = alpha
	local legal = 0
	for i = 1, n do
		local m = pick(list, scores, i, n)
		local to = (m // 128) % 128
		local quiet = board[to] == EMPTY and (m // 16384) % 8 == 0 and m // 131072 ~= 2
		local mover = board[m % 128]
		-- Futility: a quiet move at the leaves can't lift a hopeless score.
		local skip = false
		if quiet and legal > 0 and not inCheck and not pvNode and depth <= 2 and static + 150 * depth <= alpha then
			skip = true
		end
		if not skip and pos:make(m) then
			legal += 1
			local score
			local givesCheck = pos:inCheck()
			if legal == 1 then
				score = -self:search(pos, depth - 1, -beta, -alpha, ply + 1, true)
			else
				local reduce = 0
				if quiet and legal > 3 and depth >= 3 and not inCheck and not givesCheck
					and m ~= self.killers1[ply + 1] and m ~= self.killers2[ply + 1] then
					reduce = if legal > 10 then 2 else 1
				end
				score = -self:search(pos, depth - 1 - reduce, -alpha - 1, -alpha, ply + 1, true)
				if score > alpha and reduce > 0 then
					score = -self:search(pos, depth - 1, -alpha - 1, -alpha, ply + 1, true)
				end
				if score > alpha and score < beta then
					score = -self:search(pos, depth - 1, -beta, -alpha, ply + 1, true)
				end
			end
			pos:unmake()
			if self.stopped then
				return 0
			end
			if score > bestScore then
				bestScore, bestMove = score, m
				if ply == 0 then
					self.rootBest, self.rootScore = m, score
				end
			end
			if score > alpha then
				alpha = score
				if score >= beta then
					if quiet then
						if self.killers1[ply + 1] ~= m then
							self.killers2[ply + 1] = self.killers1[ply + 1]
							self.killers1[ply + 1] = m
						end
						local h = mover * 128 + to + 1
						self.history[h] = math.min(self.history[h] + depth * depth, 700000)
					end
					break
				end
			end
		end
	end
	if legal == 0 then
		return if inCheck then -MATE + ply else 0
	end
	local flag = if bestScore >= beta then LOWER elseif bestScore > startAlpha then EXACT else UPPER
	self.ttA[slot], self.ttB[slot] = pos.ha, pos.hb
	self.ttMove[slot], self.ttDepth[slot] = bestMove, depth
	self.ttScore[slot], self.ttFlag[slot] = toTT(bestScore, ply), flag
	return bestScore
end

-- The line the table says is best from here.
function Searcher:line(pos, longest)
	local moves = {}
	local made = 0
	while #moves < longest do
		local slot = pos.ha % TT_SIZE + 1
		if self.ttA[slot] ~= pos.ha or self.ttB[slot] ~= pos.hb then
			break
		end
		local m = self.ttMove[slot]
		if m == 0 or pos:repeats() >= 1 then
			break
		end
		local ok = false
		for _, legal in pos:legalMoves() do
			if legal == m then
				ok = true
				break
			end
		end
		if not ok then
			break
		end
		table.insert(moves, pos:san(m))
		pos:make(m)
		made += 1
	end
	for _ = 1, made do
		pos:unmake()
	end
	return moves
end

-- Turns a score into words for the screen: "+1.25", "-0.40", "M3", "-M2".
function Engine.scoreText(score)
	if score > MATE - 1000 then
		return "M" .. (MATE - score + 1) // 2
	elseif score < -MATE + 1000 then
		return "-M" .. (MATE + score + 1) // 2
	end
	return string.format("%+.2f", score / 100)
end

-- Thinks about `pos` and returns the move it would play (a number) and
-- what it found. Options: time (seconds), depth, noise, slip, book
-- (the moves so far, as "e2e4 e7e5"), yield (called between slices) and
-- onInfo (called after each depth with what it found).
function Searcher:think(pos, options)
	options = options or {}
	local legal = pos:legalMoves()
	if #legal == 0 then
		return nil, nil
	end
	local started = os.clock()
	self.deadline = started + (options.time or 2)
	self.sliceStart = started
	self.yield = options.yield
	self.stopped = false
	self.nodes = 0
	for i = 1, 128 do
		self.killers1[i], self.killers2[i] = 0, 0
	end
	for i = 1, #self.history do
		self.history[i] //= 8
	end

	-- From the opening book.
	if options.book then
		local choices = BOOK[options.book]
		if choices then
			local m = pos:moveFromUci(choices[math.random(#choices)])
			if m then
				return m, { depth = 0, score = 0, nodes = 0, line = { pos:san(m) }, book = true, time = 0 }
			end
		end
	end
	if #legal == 1 then
		return legal[1], { depth = 0, score = 0, nodes = 0, line = { pos:san(legal[1]) }, time = 0 }
	end

	-- A slip: a weak player sometimes moves without really looking.
	if (options.slip or 0) > 0 and math.random() < options.slip then
		local m = legal[math.random(#legal)]
		return m, { depth = 0, score = 0, nodes = 0, line = { pos:san(m) }, time = 0 }
	end

	local maxDepth = options.depth or 64
	local noise = options.noise or 0
	local best, info = legal[1], nil
	for depth = 1, maxDepth do
		self.rootBest = nil
		local score
		if noise > 0 and depth == maxDepth then
			-- Judge every move with a blur, so weaker players choose like people do.
			local bestNoisy = -INFINITE
			for _, m in legal do
				if pos:make(m) then
					local s = -self:search(pos, depth - 1, -INFINITE, INFINITE, 1, true)
					pos:unmake()
					if self.stopped then
						break
					end
					local noisy = s + (math.random() * 2 - 1) * noise
					if noisy > bestNoisy then
						bestNoisy, best, score = noisy, m, s
					end
				end
			end
			self.rootBest = best
		else
			score = self:search(pos, depth, -INFINITE, INFINITE, 0, true)
		end
		if self.rootBest then
			best = self.rootBest
			score = if self.stopped then self.rootScore else score
		end
		if self.stopped and depth > 1 then
			break
		end
		local elapsed = os.clock() - started
		info = {
			depth = depth,
			score = score or 0,
			nodes = self.nodes,
			time = elapsed,
			line = self:line(pos, 8),
		}
		if #info.line == 0 or info.line[1] ~= pos:san(best) then
			info.line = { pos:san(best) }
		end
		if options.onInfo then
			options.onInfo(info)
		end
		if self.stopped then
			break
		end
		-- A mate found, or not enough time for another, deeper pass.
		if math.abs(score or 0) > MATE - 1000 and depth > 2 then
			break
		end
		if elapsed > (options.time or 2) * 0.45 then
			break
		end
	end
	return best, info
end

-- How sure the computer is of who is better, from 0 (black winning) to 1
-- (white winning), for a bar beside the board. `score` is from white's side.
function Engine.whiteShare(score)
	if score > MATE - 1000 then
		return 1
	elseif score < -MATE + 1000 then
		return 0
	end
	return 1 / (1 + math.exp(-score / 250))
end

return Engine
