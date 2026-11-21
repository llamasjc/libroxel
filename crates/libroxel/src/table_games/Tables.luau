-- The table server shared by the table games (checkers, Connect 4 and
-- Rey Esquinado). It seats people at tables, fills seats with computer
-- players, pairs people who are looking for a game, sends and answers
-- challenges, referees every move through the game's Rules module and
-- counts everyone's wins.
--
-- A game's Rules module gives:
--   NAME, MIN_SEATS, MAX_SEATS, LEVELS (names of the computer players)
--   new(seats)                 a fresh game for that many seats
--   turn(state)                whose turn it is (a seat number)
--   play(state, seat, move)    true, or false and why not
--   result(state)              nil, or { winner = seat or nil, reason = text }
--   view(state, seat)          what that seat (or nil, a watcher) may see
--   bot(state, seat, level, yield)  the computer's move
--   BOT_PAUSE                  seconds the computer waits before each move
--
-- Screens talk to it through `Action` (a screen asks for something) and
-- `State` (the server tells a screen what to show), both in the folder.

local Players = game:GetService("Players")
local DataStoreService = game:GetService("DataStoreService")

local Tables = {}

function Tables.run(folder, Rules)
	local Action = folder:WaitForChild("Action")
	local State = folder:WaitForChild("State")
	local store = DataStoreService:GetDataStore(Rules.NAME .. "Wins")

	local people = {} -- by UserId: { player, wins, losses, played, table, seeking }
	local tables = {} -- by id
	local challenges = {} -- by id: { id, from, to }
	local nextId = 0
	local function newId()
		nextId += 1
		return tostring(nextId)
	end

	local function toast(player, text)
		State:FireClient(player, "toast", text)
	end

	local function save(person)
		pcall(function()
			store:SetAsync("player_" .. person.player.UserId, {
				wins = person.wins,
				losses = person.losses,
				played = person.played,
			})
		end)
	end

	local function updateStats(person)
		local stats = person.player:FindFirstChild("leaderstats")
		if stats then
			stats.Wins.Value = person.wins
		end
	end

	--------------------------------------------------------------------
	-- What each screen is shown.

	local function seatOf(t, person)
		for i, seat in t.seats do
			if seat.person == person then
				return i
			end
		end
		return nil
	end

	local function tableView(t, person)
		local you = seatOf(t, person)
		local seats = {}
		for i, seat in t.seats do
			seats[i] = {
				name = seat.name,
				bot = seat.bot ~= nil,
				here = seat.person == nil or seat.person.table == t,
			}
		end
		local rematch = {}
		for i in t.seats do
			rematch[i] = t.rematch[i] == true
		end
		return {
			id = t.id,
			you = you or 0,
			seats = seats,
			status = t.status,
			turn = if t.status == "playing" then Rules.turn(t.state) else 0,
			winner = t.winner or 0,
			reason = t.reason or "",
			rematch = rematch,
			vsBot = t.vsBot,
			level = t.level or 0,
			game = Rules.view(t.state, you),
			moves = t.moves,
		}
	end

	local function sendTable(t)
		for _, seat in t.seats do
			if seat.person and seat.person.table == t then
				State:FireClient(seat.person.player, "table", tableView(t, seat.person))
			end
		end
	end

	local function statusOf(person)
		if person.table and person.table.status == "playing" then
			return "playing"
		elseif person.seeking then
			return "looking for a game"
		end
		return "in the menu"
	end

	local function lobbyView(person)
		local others, incoming, outgoing = {}, {}, {}
		for id, other in people do
			if other ~= person then
				table.insert(others, {
					id = id,
					name = other.player.Name,
					wins = other.wins,
					status = statusOf(other),
					free = not (other.table and other.table.status == "playing"),
				})
			end
		end
		table.sort(others, function(a, b)
			return a.name < b.name
		end)
		for _, c in challenges do
			local from, to = people[c.from], people[c.to]
			if from and to then
				if to == person then
					table.insert(incoming, { id = c.id, name = from.player.Name })
				elseif from == person then
					table.insert(outgoing, { id = c.id, name = to.player.Name })
				end
			end
		end
		return {
			me = {
				name = person.player.Name,
				wins = person.wins,
				losses = person.losses,
				played = person.played,
				seeking = person.seeking == true,
			},
			players = others,
			incoming = incoming,
			outgoing = outgoing,
		}
	end

	local lobbyDirty = false
	local function lobbyChanged()
		lobbyDirty = true
	end

	--------------------------------------------------------------------
	-- Tables.

	local runBots -- defined below

	local function finish(t, winner, reason)
		if t.status ~= "playing" then
			return
		end
		t.status, t.winner, t.reason = "over", winner, reason
		t.generation += 1
		for i, seat in t.seats do
			local person = seat.person
			if person then
				person.played += 1
				if winner == i then
					person.wins += 1
				elseif winner then
					person.losses += 1
				end
				save(person)
				updateStats(person)
			end
		end
		sendTable(t)
		lobbyChanged()
	end

	local function leaveTable(person)
		local t = person.table
		person.table = nil
		if not t then
			return
		end
		local i = seatOf(t, person)
		if t.status == "playing" and i then
			local playing = 0
			for _, seat in t.seats do
				if seat.person and seat.person.table == t then
					playing += 1
				end
			end
			local seat = t.seats[i]
			if #t.seats > 2 and playing > 0 then
				-- A bigger table carries on: the computer takes the seat.
				seat.bot = t.level or 2
				seat.person = nil
				seat.name = seat.name .. " (computer)"
				for _, other in t.seats do
					if other.person and other.person.table == t then
						toast(other.person.player, person.player.Name .. " left; the computer plays their cards")
					end
				end
				sendTable(t)
				runBots(t)
			else
				-- Two players: whoever stays wins.
				local winner = nil
				for j, other in t.seats do
					if j ~= i then
						winner = j
					end
				end
				-- Leaving counts as a loss for the one who left.
				finish(t, winner, person.player.Name .. " left the game")
			end
		end
		if t.status == "over" then
			sendTable(t)
		end
	end

	local function startTable(seats, vsBot, level)
		local t = {
			id = newId(),
			seats = seats,
			state = Rules.new(#seats),
			status = "playing",
			rematch = {},
			vsBot = vsBot,
			level = level,
			generation = 0,
			moves = 0,
		}
		tables[t.id] = t
		for _, seat in seats do
			if seat.person then
				leaveTable(seat.person)
				seat.person.table = t
				seat.person.seeking = nil
			end
		end
		for id, c in challenges do
			for _, seat in seats do
				if seat.person and (c.from == seat.person.player.UserId or c.to == seat.person.player.UserId) then
					challenges[id] = nil
				end
			end
		end
		sendTable(t)
		lobbyChanged()
		runBots(t)
		return t
	end

	local function seatFor(person)
		return { person = person, name = person.player.Name }
	end

	local function botSeat(level, n)
		local name = Rules.LEVELS[level] or "Computer"
		if n and n > 1 then
			name = name .. " " .. n
		end
		return { bot = level, name = name }
	end

	local function afterMove(t)
		t.moves += 1
		t.generation += 1
		local result = Rules.result(t.state)
		if result then
			finish(t, result.winner, result.reason)
		else
			sendTable(t)
			runBots(t)
		end
	end

	-- When it's a computer's turn, it moves after a short pause, so people
	-- can follow what it does.
	function runBots(t)
		if t.status ~= "playing" or t.botBusy then
			return
		end
		local seat = Rules.turn(t.state)
		local s = t.seats[seat]
		if not s or not s.bot then
			return
		end
		t.botBusy = true
		local generation = t.generation
		task.spawn(function()
			task.wait(Rules.BOT_PAUSE or 0.5)
			local move = nil
			if t.status == "playing" and t.generation == generation then
				move = Rules.bot(t.state, seat, s.bot, function()
					task.wait()
				end)
			end
			t.botBusy = false
			if t.status ~= "playing" or t.generation ~= generation then
				runBots(t)
				return
			end
			if move ~= nil and Rules.play(t.state, seat, move) then
				afterMove(t)
			else
				warn(Rules.NAME .. ": the computer found no move")
			end
		end)
	end

	local function tryPairing()
		local waiting = {}
		for _, person in people do
			if person.seeking then
				table.insert(waiting, person)
			end
		end
		if #waiting < 2 then
			return false
		end
		-- Bigger tables start with everyone waiting, up to the seats there are.
		local count = math.min(#waiting, Rules.MAX_SEATS)
		if Rules.MAX_SEATS == 2 then
			count = 2
		end
		local seats = {}
		for i = 1, count do
			table.insert(seats, seatFor(waiting[i]))
		end
		-- Who goes first is up to chance.
		for i = #seats, 2, -1 do
			local j = math.random(1, i)
			seats[i], seats[j] = seats[j], seats[i]
		end
		startTable(seats, false, nil)
		return true
	end

	--------------------------------------------------------------------
	-- What screens ask for.

	local handlers = {}

	function handlers.hello(person)
		State:FireClient(person.player, "lobby", lobbyView(person))
		if person.table then
			State:FireClient(person.player, "table", tableView(person.table, person))
		end
	end

	function handlers.playBot(person, data)
		local level = math.clamp(math.floor(tonumber(data.level) or 2), 1, #Rules.LEVELS)
		local opponents = math.clamp(math.floor(tonumber(data.opponents) or 1), Rules.MIN_SEATS - 1, Rules.MAX_SEATS - 1)
		local seats = { seatFor(person) }
		for n = 1, opponents do
			table.insert(seats, botSeat(level, if opponents > 1 then n else nil))
		end
		-- Against the computer, you go first unless you ask otherwise.
		if data.first == "computer" then
			local me = table.remove(seats, 1)
			table.insert(seats, me)
		end
		startTable(seats, true, level)
	end

	function handlers.seek(person)
		if person.table and person.table.status == "playing" then
			return
		end
		person.seeking = true
		if not tryPairing() then
			toast(person.player, "Looking for someone to play…")
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
		for _, c in challenges do
			if c.from == person.player.UserId and c.to == target.player.UserId then
				toast(person.player, "You already asked " .. target.player.Name)
				return
			end
		end
		local id = newId()
		challenges[id] = { id = id, from = person.player.UserId, to = target.player.UserId }
		toast(target.player, person.player.Name .. " wants to play " .. Rules.TITLE .. " with you!")
		toast(person.player, "Asked " .. target.player.Name .. " to play")
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
			local a, b = seatFor(from), seatFor(person)
			if math.random() < 0.5 then
				a, b = b, a
			end
			startTable({ a, b }, false, nil)
		elseif c.to == person.player.UserId or c.from == person.player.UserId then
			challenges[c.id] = nil
			if from and c.to == person.player.UserId then
				toast(from.player, person.player.Name .. " said no this time")
			end
			lobbyChanged()
		end
	end

	function handlers.move(person, data)
		local t = person.table
		if not t or t.status ~= "playing" or t.id ~= data.table then
			return
		end
		local seat = seatOf(t, person)
		if not seat or Rules.turn(t.state) ~= seat then
			return
		end
		local ok, why = Rules.play(t.state, seat, data.move)
		if not ok then
			toast(person.player, why or "You can't do that")
			sendTable(t)
			return
		end
		afterMove(t)
	end

	function handlers.rematch(person)
		local t = person.table
		if not t or t.status ~= "over" then
			return
		end
		local mine = seatOf(t, person)
		if t.vsBot then
			-- The same table again; the seats turn round so the start changes hands.
			local seats = {}
			for i = 2, #t.seats do
				table.insert(seats, t.seats[i])
			end
			table.insert(seats, t.seats[1])
			local fresh = {}
			for _, seat in seats do
				if seat.person == person then
					table.insert(fresh, seatFor(person))
				elseif seat.bot then
					table.insert(fresh, { bot = seat.bot, name = seat.name })
				end
			end
			if #fresh >= 2 then
				startTable(fresh, true, t.level)
			end
			return
		end
		t.rematch[mine] = true
		local everyone = true
		for i, seat in t.seats do
			if seat.person and seat.person.table ~= t then
				toast(person.player, seat.name .. " has left the table")
				return
			end
			if seat.person and not t.rematch[i] then
				everyone = false
			end
		end
		if everyone then
			local seats = {}
			for i = 2, #t.seats do
				table.insert(seats, seatFor(t.seats[i].person))
			end
			table.insert(seats, seatFor(t.seats[1].person))
			startTable(seats, false, nil)
		else
			sendTable(t)
		end
	end

	function handlers.leave(person)
		leaveTable(person)
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

	--------------------------------------------------------------------
	-- Players arriving and leaving.

	local function join(player)
		local person = { player = player, wins = 0, losses = 0, played = 0 }
		local ok, data = pcall(function()
			return store:GetAsync("player_" .. player.UserId)
		end)
		if ok and type(data) == "table" then
			person.wins = tonumber(data.wins) or 0
			person.losses = tonumber(data.losses) or 0
			person.played = tonumber(data.played) or 0
		end
		local stats = Instance.new("Folder")
		stats.Name = "leaderstats"
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
		leaveTable(person)
		for id, c in challenges do
			if c.from == player.UserId or c.to == player.UserId then
				challenges[id] = nil
			end
		end
		people[player.UserId] = nil
		lobbyChanged()
	end)

	--------------------------------------------------------------------
	-- Old tables cleared away, and menus kept up to date.

	while true do
		task.wait(0.2)
		for id, t in tables do
			local anyone = false
			for _, seat in t.seats do
				if seat.person and seat.person.table == t then
					anyone = true
				end
			end
			if not anyone then
				t.status = "gone"
				t.generation += 1
				tables[id] = nil
			end
		end
		if lobbyDirty then
			lobbyDirty = false
			for _, person in people do
				State:FireClient(person.player, "lobby", lobbyView(person))
			end
		end
	end
end

return Tables
