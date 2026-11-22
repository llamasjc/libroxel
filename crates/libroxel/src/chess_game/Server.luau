-- The chess server: it seats players, pairs people who are looking for a
-- game, sends and answers challenges, referees every move, runs the clocks,
-- plays the computer's moves and keeps everyone's rating.
--
-- Screens talk to it through two remote events in ReplicatedStorage.Chess:
-- `Action` (a screen asks for something) and `State` (the server tells a
-- screen what it should show).

local Players = game:GetService("Players")
local storage = game:GetService("ReplicatedStorage")
local DataStoreService = game:GetService("DataStoreService")

local folder = storage:WaitForChild("Chess")
local Rules = require(folder:WaitForChild("Rules"))
local Engine = require(folder:WaitForChild("Engine"))
local Action = folder:WaitForChild("Action")
local State = folder:WaitForChild("State")

local ratingStore = DataStoreService:GetDataStore("ChessRatings")

-- Time controls: minutes and seconds added per move. 0 minutes is no clock.
local CONTROLS = {
	["none"] = { 0, 0 },
	["1+0"] = { 1, 0 },
	["3+2"] = { 3, 2 },
	["5+0"] = { 5, 0 },
	["10+0"] = { 10, 0 },
	["15+10"] = { 15, 10 },
}
local PHRASES = { "Hi!", "Good luck!", "Have fun!", "Nice move!", "Oops", "Thanks!", "Good game!", "Rematch?" }
local FIRST_MOVE_SECONDS = 30

local people = {} -- by UserId: { player, rating, wins, losses, draws, played, game, watching, seeking }
local games = {} -- by id
local challenges = {} -- by id: { id, from, to, control, colour }
local nextId = 0
local function newId()
	nextId += 1
	return tostring(nextId)
end

local function toast(player, text)
	State:FireClient(player, "toast", text)
end

-- Ratings: Elo, starting at 1200; quicker to move in the first games.
local function expected(a, b)
	return 1 / (1 + 10 ^ ((b - a) / 400))
end
local function kFactor(person)
	return if person.played < 20 then 40 else 24
end

local function save(person)
	local data = {
		rating = person.rating,
		wins = person.wins,
		losses = person.losses,
		draws = person.draws,
		played = person.played,
		botWins = person.botWins,
	}
	pcall(function()
		ratingStore:SetAsync("player_" .. person.player.UserId, data)
	end)
end

local function updateStats(person)
	local stats = person.player:FindFirstChild("leaderstats")
	if stats then
		stats.Rating.Value = person.rating
		stats.Wins.Value = person.wins + person.botWins
	end
end

------------------------------------------------------------------------
-- What each screen is shown.

local function clockNow(g, side)
	local left = g.clocks[side]
	if g.status == "playing" and g.running == side then
		left -= (os.clock() - g.turnStarted) * 1000
	end
	return math.max(0, math.floor(left))
end

local function seatView(seat)
	return {
		name = seat.name,
		rating = seat.rating,
		bot = seat.bot ~= nil,
		colour = seat.colour,
		id = if seat.person then seat.person.player.UserId else 0,
	}
end

local function gameView(g, viewer)
	local you = "watching"
	if g.white.person == viewer then
		you = "white"
	elseif g.black.person == viewer then
		you = "black"
	end
	local chat = {}
	for i = math.max(1, #g.chat - 6), #g.chat do
		table.insert(chat, g.chat[i])
	end
	return {
		id = g.id,
		you = you,
		white = seatView(g.white),
		black = seatView(g.black),
		fen = g.pos:fen(),
		sans = table.clone(g.sans),
		last = g.moves[#g.moves] or "",
		ply = #g.moves,
		status = g.status,
		result = g.result or "",
		reason = g.reason or "",
		control = g.control,
		timed = g.timed,
		clocks = { clockNow(g, 0), clockNow(g, 1) },
		running = if g.status == "playing" and g.running then g.running else -1,
		drawOffer = g.drawOffer or -1,
		rematch = { g.rematch[0] == true, g.rematch[1] == true },
		rated = g.rated,
		vsBot = g.white.bot ~= nil or g.black.bot ~= nil,
		thinking = g.thinking == true,
		info = g.info and {
			depth = g.info.depth,
			score = g.info.score,
			nodes = g.info.nodes,
			time = g.info.time,
			line = table.clone(g.info.line),
			by = g.info.by,
			book = g.info.book == true,
		} or false,
		change = g.change and { g.change[0], g.change[1] } or false,
		chat = chat,
		hint = (viewer and g.hints and g.hints[viewer.player.UserId]) or "",
		firstMoveLeft = if g.status == "playing" and #g.moves < 2
			then math.max(0, math.floor(FIRST_MOVE_SECONDS - (os.clock() - g.turnStarted)))
			else -1,
	}
end

-- Sends a game to everyone at it: both players and everyone watching.
local function sendGame(g)
	for _, seat in { g.white, g.black } do
		if seat.person and seat.person.game == g then
			State:FireClient(seat.person.player, "game", gameView(g, seat.person))
		end
	end
	for id in g.watchers do
		local person = people[id]
		if person and person.watching == g then
			State:FireClient(person.player, "game", gameView(g, person))
		end
	end
end

local function statusOf(person)
	if person.game and person.game.status == "playing" then
		return "playing"
	elseif person.seeking then
		return "looking for " .. person.seeking
	end
	return "in the lobby"
end

local function lobbyView(person)
	local players, live, incoming, outgoing = {}, {}, {}, {}
	for id, other in people do
		if other ~= person then
			table.insert(players, {
				id = id,
				name = other.player.Name,
				rating = other.rating,
				status = statusOf(other),
				free = not (other.game and other.game.status == "playing"),
			})
		end
	end
	table.sort(players, function(a, b)
		return a.rating > b.rating
	end)
	for _, g in games do
		if g.status == "playing" then
			table.insert(live, {
				id = g.id,
				white = g.white.name,
				black = g.black.name,
				whiteRating = g.white.rating,
				blackRating = g.black.rating,
				control = g.control,
				ply = #g.moves,
				mine = g.white.person == person or g.black.person == person,
			})
		end
	end
	table.sort(live, function(a, b)
		return (a.whiteRating + a.blackRating) > (b.whiteRating + b.blackRating)
	end)
	for _, c in challenges do
		local from, to = people[c.from], people[c.to]
		if from and to then
			if to == person then
				table.insert(incoming, { id = c.id, name = from.player.Name, rating = from.rating, control = c.control, colour = c.colour })
			elseif from == person then
				table.insert(outgoing, { id = c.id, name = to.player.Name, rating = to.rating, control = c.control, colour = c.colour })
			end
		end
	end
	return {
		me = {
			name = person.player.Name,
			rating = person.rating,
			wins = person.wins,
			losses = person.losses,
			draws = person.draws,
			botWins = person.botWins,
			seeking = person.seeking or "",
		},
		players = players,
		live = live,
		incoming = incoming,
		outgoing = outgoing,
	}
end

local lobbyDirty = false
local function sendLobbies()
	lobbyDirty = false
	for _, person in people do
		State:FireClient(person.player, "lobby", lobbyView(person))
	end
end
local function lobbyChanged()
	lobbyDirty = true
end

------------------------------------------------------------------------
-- Games.

local playBot -- defined below

local function finish(g, result, reason)
	if g.status ~= "playing" then
		return
	end
	if g.running then
		g.clocks[g.running] = clockNow(g, g.running)
	end
	g.status, g.result, g.reason, g.running = "over", result, reason, nil
	g.thinking = false
	g.drawOffer = nil
	local w, b = g.white.person, g.black.person
	local score = if result == "1-0" then 1 elseif result == "0-1" then 0 else 0.5
	if reason == "aborted" then
		score = nil
	end
	if score and w and b then
		w.played += 1
		b.played += 1
		if score == 1 then
			w.wins += 1
			b.losses += 1
		elseif score == 0 then
			b.wins += 1
			w.losses += 1
		else
			w.draws += 1
			b.draws += 1
		end
		if g.rated then
			local ew = expected(w.rating, b.rating)
			local dw = math.floor(kFactor(w) * (score - ew) + 0.5)
			local db = math.floor(kFactor(b) * ((1 - score) - (1 - ew)) + 0.5)
			w.rating += dw
			b.rating += db
			g.change = { [0] = dw, [1] = db }
		end
		save(w)
		save(b)
		updateStats(w)
		updateStats(b)
	elseif score then
		-- Against the computer: count the wins.
		local person = w or b
		if person then
			local mine = if w then 1 else 0
			if (mine == 1 and score == 1) or (mine == 0 and score == 0) then
				person.botWins += 1
				save(person)
				updateStats(person)
			end
		end
	end
	sendGame(g)
	lobbyChanged()
end

local function sideOf(g, person)
	if g.white.person == person then
		return 0
	elseif g.black.person == person then
		return 1
	end
	return nil
end

local function seatFor(person)
	return { person = person, name = person.player.Name, rating = person.rating }
end

local function botSeat(level)
	local bot = Engine.BOTS[level]
	return { bot = level, name = bot.name, rating = bot.rating, colour = bot.colour }
end

local function leaveGame(person)
	local g = person.game
	if g and g.status == "playing" then
		local side = sideOf(g, person)
		if #g.moves < 2 then
			finish(g, "*", "aborted")
		else
			finish(g, if side == 0 then "0-1" else "1-0", "resignation")
		end
	end
	person.game = nil
	if person.watching then
		person.watching.watchers[person.player.UserId] = nil
		person.watching = nil
	end
end

local function startGame(white, black, controlName, rated)
	local control = CONTROLS[controlName] or CONTROLS["none"]
	local timed = control[1] > 0
	local g = {
		id = newId(),
		white = white,
		black = black,
		pos = Rules.new(),
		moves = {},
		sans = {},
		control = controlName,
		timed = timed,
		increment = control[2] * 1000,
		clocks = { [0] = control[1] * 60000, [1] = control[1] * 60000 },
		running = nil,
		turnStarted = os.clock(),
		status = "playing",
		rated = rated,
		rematch = {},
		watchers = {},
		chat = {},
		hints = {},
		generation = 0,
	}
	games[g.id] = g
	for _, seat in { white, black } do
		if seat.person then
			leaveGame(seat.person)
			seat.person.game = g
			seat.person.seeking = nil
		end
	end
	-- Challenges to or from either player are off now.
	for id, c in challenges do
		for _, seat in { white, black } do
			if seat.person and (c.from == seat.person.player.UserId or c.to == seat.person.player.UserId) then
				challenges[id] = nil
			end
		end
	end
	sendGame(g)
	lobbyChanged()
	playBot(g)
	return g
end

-- Plays `m` (a legal move number) for the side to move.
local function applyMove(g, m)
	local pos = g.pos
	local side = pos.side
	local san = pos:san(m)
	pos:make(m)
	table.insert(g.moves, Rules.uci(m))
	table.insert(g.sans, san)
	g.generation += 1
	g.drawOffer = if g.drawOffer == side then side else nil
	g.hints = {}
	local now = os.clock()
	if g.timed and g.running == side then
		g.clocks[side] = g.clocks[side] - (now - g.turnStarted) * 1000 + g.increment
	end
	g.turnStarted = now
	-- The clocks start once both sides have moved.
	if g.timed and #g.moves >= 2 then
		g.running = 1 - side
	end
	local result, reason = pos:outcome()
	if result then
		finish(g, result, reason)
	else
		sendGame(g)
		playBot(g)
	end
	if #g.moves <= 2 then
		lobbyChanged()
	end
end

-- A fresh copy of a game's position, with its history, for the computer
-- to think about while the real one stays free.
local function copyPosition(g)
	local pos = Rules.new()
	for _, uci in g.moves do
		pos:make(pos:moveFromUci(uci))
	end
	return pos
end

local searchers = {}
local function borrowSearcher()
	return table.remove(searchers) or Engine.new()
end

function playBot(g)
	if g.status ~= "playing" then
		return
	end
	local side = g.pos.side
	local seat = if side == 0 then g.white else g.black
	if not seat.bot then
		return
	end
	local level = Engine.BOTS[seat.bot]
	local generation = g.generation
	g.thinking = true
	task.spawn(function()
		local started = os.clock()
		local pos = copyPosition(g)
		local searcher = borrowSearcher()
		local budget = level.time
		if g.timed then
			local left = g.clocks[side] / 1000
			budget = math.min(budget, left / 30 + g.increment / 1000 * 0.8)
			budget = math.max(budget, 0.15)
		end
		local lastSent = 0
		local move, info = searcher:think(pos, {
			time = budget,
			depth = level.depth,
			noise = level.noise,
			slip = if #g.moves >= 4 then level.slip else 0,
			book = if #g.moves < 6 then table.concat(g.moves, " ") else nil,
			yield = function()
				task.wait()
			end,
			onInfo = function(found)
				if g.generation ~= generation then
					return
				end
				g.info = {
					depth = found.depth,
					score = if side == 0 then found.score else -found.score,
					nodes = found.nodes,
					time = found.time,
					line = found.line,
					by = level.name,
				}
				if os.clock() - lastSent > 0.25 then
					lastSent = os.clock()
					sendGame(g)
				end
			end,
		})
		table.insert(searchers, searcher)
		if g.status ~= "playing" or g.generation ~= generation then
			return
		end
		if info then
			g.info = {
				depth = info.depth,
				score = if side == 0 then info.score else -info.score,
				nodes = info.nodes,
				time = info.time,
				line = info.line,
				by = level.name,
				book = info.book,
			}
		end
		-- Weaker players take a moment, as if thinking it over.
		local least = if g.timed then 0.2 else 0.6
		local spent = os.clock() - started
		if spent < least then
			task.wait(least - spent)
		end
		if g.status ~= "playing" or g.generation ~= generation or not move then
			return
		end
		g.thinking = false
		applyMove(g, g.pos:moveFromUci(Rules.uci(move)))
	end)
end

------------------------------------------------------------------------
-- Looking for a game.

local function tryPairing(person)
	for _, other in people do
		if other ~= person and other.seeking == person.seeking then
			local control = person.seeking
			local a, b = seatFor(person), seatFor(other)
			if math.random() < 0.5 then
				a, b = b, a
			end
			startGame(a, b, control, true)
			return true
		end
	end
	return false
end

------------------------------------------------------------------------
-- What screens ask for.

local handlers = {}

function handlers.hello(person)
	State:FireClient(person.player, "lobby", lobbyView(person))
	local g = person.game or person.watching
	if g then
		State:FireClient(person.player, "game", gameView(g, person))
	end
end

function handlers.playBot(person, data)
	local level = tonumber(data.level)
	if not level or not Engine.BOTS[level] then
		return
	end
	local control = if CONTROLS[data.control] then data.control else "none"
	local colour = data.colour
	if colour ~= "white" and colour ~= "black" then
		colour = if math.random() < 0.5 then "white" else "black"
	end
	local me, bot = seatFor(person), botSeat(level)
	if colour == "white" then
		startGame(me, bot, control, false)
	else
		startGame(bot, me, control, false)
	end
end

function handlers.seek(person, data)
	if person.game and person.game.status == "playing" then
		return
	end
	local control = if CONTROLS[data.control] then data.control else "5+0"
	person.seeking = control
	if not tryPairing(person) then
		toast(person.player, "Searching " .. (if control == "none" then "no clock" else control) .. "…")
	end
	lobbyChanged()
end

function handlers.cancelSeek(person)
	person.seeking = nil
	lobbyChanged()
end

function handlers.challenge(person, data)
	local target = people[tonumber(data.to) or -1]
	if not target or target == person then
		return
	end
	local control = if CONTROLS[data.control] then data.control else "5+0"
	local colour = if data.colour == "white" or data.colour == "black" then data.colour else "random"
	for _, c in challenges do
		if c.from == person.player.UserId and c.to == target.player.UserId then
			toast(person.player, "You already challenged " .. target.player.Name)
			return
		end
	end
	local id = newId()
	challenges[id] = { id = id, from = person.player.UserId, to = target.player.UserId, control = control, colour = colour }
	toast(target.player, person.player.Name .. " (" .. person.rating .. ") challenges you to a " .. control .. " game!")
	toast(person.player, "Challenge sent to " .. target.player.Name)
	lobbyChanged()
end

function handlers.answer(person, data)
	local c = challenges[tostring(data.id)]
	if not c then
		return
	end
	local from = people[c.from]
	if data.accept and c.to == person.player.UserId and from then
		challenges[c.id] = nil
		local white, black = seatFor(from), seatFor(person)
		if c.colour == "black" or (c.colour == "random" and math.random() < 0.5) then
			white, black = black, white
		end
		startGame(white, black, c.control, true)
	elseif c.to == person.player.UserId or c.from == person.player.UserId then
		challenges[c.id] = nil
		if from and c.to == person.player.UserId then
			toast(from.player, person.player.Name .. " declined your challenge")
		end
		lobbyChanged()
	end
end

function handlers.move(person, data)
	local g = person.game
	if not g or g.status ~= "playing" or g.id ~= data.game then
		return
	end
	if sideOf(g, person) ~= g.pos.side then
		return
	end
	local m = g.pos:moveFromUci(data.uci)
	if not m then
		toast(person.player, "That move isn't allowed")
		sendGame(g)
		return
	end
	applyMove(g, m)
end

function handlers.resign(person)
	local g = person.game
	if not g or g.status ~= "playing" then
		return
	end
	if #g.moves < 2 then
		finish(g, "*", "aborted")
	else
		finish(g, if sideOf(g, person) == 0 then "0-1" else "1-0", "resignation")
	end
end

function handlers.draw(person)
	local g = person.game
	if not g or g.status ~= "playing" then
		return
	end
	local side = sideOf(g, person)
	local other = if side == 0 then g.black else g.white
	if other.bot then
		-- The computer takes a draw only when it thinks it is worse off.
		local score = g.info and g.info.score or 0
		local forBot = if side == 0 then -score else score
		if forBot < -150 and #g.moves >= 20 then
			finish(g, "1/2-1/2", "agreement")
		else
			toast(person.player, other.name .. " declines the draw")
		end
		return
	end
	if g.drawOffer == 1 - side then
		finish(g, "1/2-1/2", "agreement")
	else
		g.drawOffer = side
		sendGame(g)
	end
end

function handlers.declineDraw(person)
	local g = person.game
	if g and g.drawOffer and g.drawOffer ~= sideOf(g, person) then
		g.drawOffer = nil
		sendGame(g)
	end
end

-- Against the computer, take back your last move (and its answer).
function handlers.takeback(person)
	local g = person.game
	if not g or g.status ~= "playing" then
		return
	end
	local side = sideOf(g, person)
	local other = if side == 0 then g.black else g.white
	if not other.bot then
		return
	end
	local undo = if g.pos.side == side then 2 else 1
	if #g.moves < undo or (undo == 2 and #g.moves < 2) then
		return
	end
	for _ = 1, undo do
		table.remove(g.moves)
		table.remove(g.sans)
		g.pos:unmake()
	end
	g.generation += 1
	g.thinking = false
	g.turnStarted = os.clock()
	g.running = if g.timed and #g.moves >= 2 then g.pos.side else nil
	g.hints = {}
	sendGame(g)
	playBot(g)
end

-- Against the computer, ask it which move it would play.
function handlers.hint(person)
	local g = person.game
	if not g or g.status ~= "playing" or sideOf(g, person) ~= g.pos.side then
		return
	end
	local generation = g.generation
	task.spawn(function()
		local searcher = borrowSearcher()
		local move = searcher:think(copyPosition(g), {
			time = 1,
			yield = function()
				task.wait()
			end,
		})
		table.insert(searchers, searcher)
		if move and g.generation == generation then
			g.hints[person.player.UserId] = Rules.uci(move)
			State:FireClient(person.player, "game", gameView(g, person))
		end
	end)
end

function handlers.rematch(person)
	local g = person.game
	if not g or g.status ~= "over" then
		return
	end
	local side = sideOf(g, person)
	local other = if side == 0 then g.black else g.white
	if other.bot then
		local me, bot = seatFor(person), botSeat(other.bot)
		if side == 0 then
			startGame(bot, me, g.control, false)
		else
			startGame(me, bot, g.control, false)
		end
		return
	end
	if not (other.person and other.person.game == g) then
		toast(person.player, other.name .. " has left the game")
		return
	end
	g.rematch[side] = true
	if g.rematch[1 - side] then
		startGame(seatFor(g.black.person), seatFor(g.white.person), g.control, g.rated)
	else
		sendGame(g)
	end
end

function handlers.chat(person, data)
	local g = person.game
	local phrase = PHRASES[tonumber(data.phrase) or 0]
	if not g or not phrase then
		return
	end
	table.insert(g.chat, person.player.Name .. ": " .. phrase)
	sendGame(g)
end

function handlers.watch(person, data)
	local g = games[tostring(data.game)]
	if not g then
		return
	end
	if person.game and person.game.status == "playing" then
		toast(person.player, "Finish your game first")
		return
	end
	leaveGame(person)
	person.watching = g
	g.watchers[person.player.UserId] = true
	State:FireClient(person.player, "game", gameView(g, person))
end

function handlers.leave(person)
	leaveGame(person)
	State:FireClient(person.player, "left")
	lobbyChanged()
end

Action.OnServerEvent:Connect(function(player, action, data)
	local person = people[player.UserId]
	local handler = handlers[action]
	if person and handler and type(action) == "string" then
		handler(person, if type(data) == "table" then data else {})
	end
end)

------------------------------------------------------------------------
-- Players arriving and leaving.

local function join(player)
	local person = { player = player, rating = 1200, wins = 0, losses = 0, draws = 0, played = 0, botWins = 0 }
	local ok, data = pcall(function()
		return ratingStore:GetAsync("player_" .. player.UserId)
	end)
	if ok and type(data) == "table" then
		person.rating = tonumber(data.rating) or 1200
		person.wins = tonumber(data.wins) or 0
		person.losses = tonumber(data.losses) or 0
		person.draws = tonumber(data.draws) or 0
		person.played = tonumber(data.played) or 0
		person.botWins = tonumber(data.botWins) or 0
	end
	local stats = Instance.new("Folder")
	stats.Name = "leaderstats"
	local rating = Instance.new("IntValue")
	rating.Name = "Rating"
	rating.Parent = stats
	local wins = Instance.new("IntValue")
	wins.Name = "Wins"
	wins.Parent = stats
	stats.Parent = player
	people[player.UserId] = person
	updateStats(person)
	lobbyChanged()
end

Players.PlayerAdded:Connect(join)
for _, player in Players:GetPlayers() do
	if not people[player.UserId] then
		join(player)
	end
end

Players.PlayerRemoving:Connect(function(player)
	local person = people[player.UserId]
	if not person then
		return
	end
	leaveGame(person)
	for id, c in challenges do
		if c.from == player.UserId or c.to == player.UserId then
			challenges[id] = nil
		end
	end
	people[player.UserId] = nil
	lobbyChanged()
end)

------------------------------------------------------------------------
-- The referee's watch: flags falling, first moves not made, old games
-- cleared away, and lobbies kept up to date.

while true do
	task.wait(0.1)
	local now = os.clock()
	for id, g in games do
		if g.status == "playing" then
			if g.running and clockNow(g, g.running) <= 0 then
				local side = g.running
				g.clocks[side] = 0
				local winner = 1 - side
				if g.pos:canMate(winner) then
					finish(g, if winner == 0 then "1-0" else "0-1", "time")
				else
					finish(g, "1/2-1/2", "time, with too little to mate")
				end
			elseif #g.moves < 2 and now - g.turnStarted > FIRST_MOVE_SECONDS then
				local seat = if g.pos.side == 0 then g.white else g.black
				if not seat.bot then
					finish(g, "*", "aborted")
				end
			end
		else
			-- A finished game stays while someone is still looking at it.
			local anyone = false
			for _, seat in { g.white, g.black } do
				if seat.person and seat.person.game == g then
					anyone = true
				end
			end
			for watcher in g.watchers do
				if people[watcher] and people[watcher].watching == g then
					anyone = true
				end
			end
			if not anyone then
				games[id] = nil
			end
		end
	end
	if lobbyDirty then
		sendLobbies()
	end
end
