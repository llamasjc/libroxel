-- Kings in the Corner, for 2 to 4 players with one
-- 52-card deck.
--
-- Each player gets 7 cards and four cards go face up beside the draw pile,
-- one on each side (north, east, south, west). The corners start empty.
-- On your turn you first draw a card, then play as much as you like:
--   * a card onto a pile, if it is one lower and the other colour
--     (red on black, black on red: a black 6 goes on a red 7);
--   * a King into an empty corner;
--   * a whole side pile onto another pile, if its bottom card fits on
--     that pile's top card; the side spot it leaves is empty;
--   * any card into an empty side spot.
-- Then end your turn. The first to play every card wins. If the draw pile
-- is gone and a whole round goes by with nobody playing, whoever has the
-- fewest cards wins.
--
-- Cards are numbered 1 to 52: rank (card - 1) % 13 + 1 (1 is the Ace,
-- 13 the King) and suit (card - 1) // 13: spades, hearts, diamonds, clubs.
-- Piles 1 to 4 are the sides (north, east, south, west) and 5 to 8 the
-- corners (north-east, south-east, south-west, north-west).

local Rules = {}

Rules.NAME = "KingsInTheCorner"
Rules.TITLE = "Kings in the Corner"
Rules.MIN_SEATS = 2
Rules.MAX_SEATS = 4
Rules.LEVELS = { "Easy", "Medium", "Hard" }
Rules.BOT_PAUSE = 0.8
Rules.HAND = 7

Rules.SUITS = { "♠", "♥", "♦", "♣" }
Rules.RANKS = { "A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K" }
Rules.PILE_NAMES = { "north", "east", "south", "west", "north-east corner", "south-east corner", "south-west corner", "north-west corner" }

local function rankOf(card)
	return (card - 1) % 13 + 1
end
local function suitOf(card)
	return (card - 1) // 13 + 1
end
local function isRed(card)
	local suit = suitOf(card)
	return suit == 2 or suit == 3
end
Rules.rankOf, Rules.suitOf, Rules.isRed = rankOf, suitOf, isRed

function Rules.name(card)
	return Rules.RANKS[rankOf(card)] .. Rules.SUITS[suitOf(card)]
end

local function isCorner(pile)
	return pile >= 5
end
Rules.isCorner = isCorner

-- Can `card` go on top of `top`?
local function fits(top, card)
	return rankOf(card) == rankOf(top) - 1 and isRed(card) ~= isRed(top)
end
Rules.fits = fits

-- Can `card` (from a hand) go on pile `pile`, as it stands?
function Rules.canPlace(piles, pile, card)
	local cards = piles[pile]
	if not cards then
		return false
	end
	if #cards == 0 then
		return if isCorner(pile) then rankOf(card) == 13 else true
	end
	return fits(cards[#cards], card)
end

-- Can the whole side pile `from` go onto pile `to`?
function Rules.canMove(piles, from, to)
	if from == to or isCorner(from) or not piles[from] or not piles[to] then
		return false
	end
	local moving = piles[from]
	if #moving == 0 then
		return false
	end
	local target = piles[to]
	if #target == 0 then
		-- Only a pile with a King at the bottom moves into an empty corner.
		return isCorner(to) and rankOf(moving[1]) == 13
	end
	return fits(target[#target], moving[1])
end

local function shuffled()
	local deck = {}
	for card = 1, 52 do
		deck[card] = card
	end
	for i = 52, 2, -1 do
		local j = math.random(1, i)
		deck[i], deck[j] = deck[j], deck[i]
	end
	return deck
end

local function startTurn(state)
	state.played = false
	state.drew = 0
	if #state.stock > 0 then
		local card = table.remove(state.stock)
		table.insert(state.hands[state.turn], card)
		state.drew = card
	end
end

function Rules.new(seats, deck)
	local state = {
		seats = seats,
		stock = deck or shuffled(),
		hands = {},
		piles = { {}, {}, {}, {}, {}, {}, {}, {} },
		turn = 1,
		played = false,
		drew = 0,
		idle = 0,
		log = "",
		winner = nil,
		turns = 0,
	}
	for seat = 1, seats do
		state.hands[seat] = {}
	end
	for _ = 1, Rules.HAND do
		for seat = 1, seats do
			table.insert(state.hands[seat], table.remove(state.stock))
		end
	end
	-- One card face up on each side. A King goes to a corner instead.
	for side = 1, 4 do
		while #state.piles[side] == 0 and #state.stock > 0 do
			local card = table.remove(state.stock)
			if rankOf(card) == 13 then
				for corner = 5, 8 do
					if #state.piles[corner] == 0 then
						table.insert(state.piles[corner], card)
						break
					end
				end
			else
				table.insert(state.piles[side], card)
			end
		end
	end
	startTurn(state)
	return state
end

function Rules.turn(state)
	return state.turn
end

local function endTurn(state)
	if not state.played and #state.stock == 0 then
		state.idle += 1
	else
		state.idle = 0
	end
	state.turns += 1
	state.turn = state.turn % state.seats + 1
	startTurn(state)
end

-- move: { kind = "card", card = n, pile = p } | { kind = "pile", from = a, to = b } | { kind = "end" }
function Rules.play(state, seat, move)
	if state.turn ~= seat or state.winner then
		return false, "It isn't your turn"
	end
	if type(move) ~= "table" then
		return false, "That isn't a move"
	end
	if move.kind == "end" then
		state.log = ""
		endTurn(state)
		return true
	elseif move.kind == "card" then
		local hand = state.hands[seat]
		local at = table.find(hand, move.card)
		local pile = tonumber(move.pile)
		if not at or not pile then
			return false, "You don't have that card"
		end
		if not Rules.canPlace(state.piles, pile, move.card) then
			if isCorner(pile) and #state.piles[pile] == 0 then
				return false, "Only a King can start a corner"
			end
			return false, "It has to be one lower and the other colour"
		end
		table.remove(hand, at)
		table.insert(state.piles[pile], move.card)
		state.played = true
		state.log = Rules.name(move.card) .. " on the " .. Rules.PILE_NAMES[pile]
		if #hand == 0 then
			state.winner = seat
		end
		return true
	elseif move.kind == "pile" then
		local from, to = tonumber(move.from), tonumber(move.to)
		if not from or not to or not Rules.canMove(state.piles, from, to) then
			return false, "That pile doesn't fit there"
		end
		for _, card in state.piles[from] do
			table.insert(state.piles[to], card)
		end
		state.piles[from] = {}
		state.played = true
		state.log = "Moved the " .. Rules.PILE_NAMES[from] .. " pile onto the " .. Rules.PILE_NAMES[to]
		return true
	end
	return false, "That isn't a move"
end

function Rules.result(state)
	if state.winner then
		return { winner = state.winner, reason = "Played every card" }
	end
	if #state.stock == 0 and state.idle >= state.seats then
		local best, fewest = nil, math.huge
		for seat = 1, state.seats do
			local count = #state.hands[seat]
			if count < fewest then
				best, fewest = seat, count
			elseif count == fewest then
				best = nil
			end
		end
		return {
			winner = best,
			reason = if best then "Nobody could play: fewest cards left" else "Nobody could play, and it's a tie",
		}
	end
	return nil
end

function Rules.view(state, seat)
	local counts = {}
	for i = 1, state.seats do
		counts[i] = #state.hands[i]
	end
	local piles = {}
	for i, pile in state.piles do
		piles[i] = table.clone(pile)
	end
	return {
		piles = piles,
		hand = if seat then table.clone(state.hands[seat]) else {},
		counts = counts,
		stock = #state.stock,
		drew = if seat == state.turn then state.drew else 0,
		log = state.log,
		played = state.played,
	}
end

------------------------------------------------------------------------
-- The computer: plays what it can, one move at a time.

-- Every move `seat` could make now, but "end".
local function options(state, seat)
	local piles = state.piles
	local out = {}
	for from = 1, 4 do
		for to = 1, 8 do
			if Rules.canMove(piles, from, to) then
				table.insert(out, { kind = "pile", from = from, to = to, order = 1 })
			end
		end
	end
	for _, card in state.hands[seat] do
		for pile = 1, 8 do
			if Rules.canPlace(piles, pile, card) then
				local empty = #piles[pile] == 0
				local order = if isCorner(pile) then 2 elseif empty then 4 else 3
				table.insert(out, { kind = "card", card = card, pile = pile, order = order })
			end
		end
	end
	return out
end
Rules.options = options

-- How many of the hand could follow if `card` went in an empty spot.
local function followers(hand, card)
	local count, top = 0, card
	local left = table.clone(hand)
	table.remove(left, table.find(left, card))
	local found = true
	while found do
		found = false
		for i, other in left do
			if fits(top, other) then
				count += 1
				top = other
				table.remove(left, i)
				found = true
				break
			end
		end
	end
	return count
end

function Rules.bot(state, seat, level)
	local moves = options(state, seat)
	if #moves == 0 then
		return { kind = "end" }
	end
	-- Easy sometimes stops early.
	if level == 1 and state.played and math.random() < 0.35 then
		return { kind = "end" }
	end
	local hand = state.hands[seat]
	local best, bestScore = nil, -math.huge
	for _, m in moves do
		local score = (5 - m.order) * 100
		if m.kind == "card" then
			local rank = rankOf(m.card)
			if m.order == 4 then
				-- Filling an empty side: a high card that lets more follow.
				score += rank + (if level >= 3 then followers(hand, m.card) * 20 else 0)
				-- Hard keeps Kings for the corners when a corner is free.
				if level >= 3 and rank == 13 then
					score -= 50
				end
			else
				score += rank
			end
		end
		if level == 1 then
			score += math.random() * 150
		end
		if score > bestScore then
			best, bestScore = m, score
		end
	end
	return { kind = best.kind, card = best.card, pile = best.pile, from = best.from, to = best.to }
end

return Rules
