-- Hide and seek. One seeker waits in the shed with their eyes shut while
-- everyone else hides round the houses. Then the seeker comes out: get
-- close to a hider with nothing in the way and they're found, and they
-- turn seeker too. Hiders win if anyone is still hidden when time's up.

local Arena = require(script.Parent:WaitForChild("Arena"))

local PLAYERS = 6
local HIDE = 20
local SEEK = 120
local BREAK = 8
local FIND_RANGE = 6
local HIDER_SPEED, SEEKER_SPEED = 16, 17
local BOT_HIDER_SPEED, BOT_SEEKER_SPEED = 15, 14
local RED = Color3.fromRGB(235, 50, 45)

local hidingSpots = Arena.spots(workspace:WaitForChild("HidingSpots"))
local shed = workspace:WaitForChild("Shed"):WaitForChild("Inside").Position
-- Seekers come out in the square.
local square = workspace:WaitForChild("Square").Position
local spawns = Arena.spots(workspace:WaitForChild("Spawns"))

Arena.score("Wins")
Arena.score("Found")

local function nearestOf(c, list, keep)
	local best, bestDistance = nil, math.huge
	for _, other in list do
		if keep(other) then
			local d = Arena.distance(c, other)
			if d < bestDistance then
				best, bestDistance = other, d
			end
		end
	end
	return best, bestDistance
end

local function playRound()
	Arena.fillWithBots(PLAYERS, square)
	task.wait(0.5)
	local people = Arena.everyone()
	if #people < 2 then
		return
	end
	for _, c in people do
		Arena.reset(c)
	end

	-- Pick the seeker (two when there are lots of people).
	Arena.shuffle(people)
	local seekers = { [people[1].model] = true }
	if #people >= 8 then
		seekers[people[2].model] = true
	end
	local hidingAt = {} -- where each hiding bot is going, by model
	local spots = Arena.shuffle(table.clone(hidingSpots))
	for i, c in people do
		if seekers[c.model] then
			Arena.role(c, "Seeker")
			Arena.mark(c, RED)
			Arena.place(c, shed)
			Arena.speed(c, 0)
			if c.player then
				c.player:SetAttribute("Blind", true)
			end
		else
			Arena.role(c, "Hider")
			Arena.place(c, spawns[(i - 1) % #spawns + 1])
			Arena.speed(c, c.bot and BOT_HIDER_SPEED or HIDER_SPEED)
			if c.bot then
				hidingAt[c.model] = spots[(i - 1) % #spots + 1]
				Arena.go(c, hidingAt[c.model])
			end
		end
	end

	-- Hiding.
	Arena.status("Hide")
	Arena.tell("Hide!")
	Arena.countdown(HIDE, function()
		for _, c in people do
			if c.bot and hidingAt[c.model] and Arena.inGame(c) then
				-- Bots that got stuck try again.
				Arena.go(c, hidingAt[c.model])
			end
		end
		return false
	end, 0.5)

	-- Seeking.
	for _, c in people do
		if seekers[c.model] and Arena.inGame(c) then
			Arena.place(c, square)
			Arena.speed(c, c.bot and BOT_SEEKER_SPEED or SEEKER_SPEED)
			if c.player then
				c.player:SetAttribute("Blind", false)
			end
		end
	end
	Arena.tell("Seeker is out")

	local roamTo = {} -- where each seeking bot is heading, by model
	local nextThink = 0
	local function left()
		local count = 0
		for _, c in people do
			if not seekers[c.model] and Arena.inGame(c) then
				count += 1
			end
		end
		return count
	end
	local allFound = Arena.countdown(SEEK, function()
		Arena.status(left() .. " hiding")
		-- Finds.
		for _, s in people do
			if seekers[s.model] and Arena.inGame(s) then
				for _, h in people do
					if not seekers[h.model] and Arena.inGame(h) then
						local across, up = Arena.distance(s, h)
						if across < FIND_RANGE and up < 6 and Arena.sees(s, h) then
							seekers[h.model] = true
							Arena.bump(s, "Found")
							Arena.role(h, "Found")
							Arena.mark(h, RED)
							Arena.speed(h, h.bot and BOT_SEEKER_SPEED or SEEKER_SPEED)
							Arena.tell(s.name .. " found " .. h.name)
						end
					end
				end
			end
		end

		-- Bots.
		if time() >= nextThink then
			nextThink = time() + 0.3
			for _, c in people do
				if c.bot and Arena.inGame(c) then
					if seekers[c.model] then
						local target, d = nearestOf(c, people, function(o)
							return not seekers[o.model] and Arena.inGame(o)
						end)
						if target and d < 30 and Arena.sees(c, target) then
							Arena.walk(c, target.root.Position)
						else
							local goal = roamTo[c.model]
							if not goal or (c.root.Position - goal).Magnitude < 4 or math.random() < 0.03 then
								goal = hidingSpots[math.random(#hidingSpots)]
								roamTo[c.model] = goal
							end
							Arena.go(c, goal)
						end
					else
						-- A hiding bot stays put, unless a seeker is coming its way.
						local seeker, d = nearestOf(c, people, function(o)
							return seekers[o.model] and Arena.inGame(o)
						end)
						if seeker and d < 14 and Arena.sees(seeker, c) then
							local far, farthest = hidingAt[c.model], 0
							for _ = 1, 4 do
								local spot = hidingSpots[math.random(#hidingSpots)]
								local away = (spot - seeker.root.Position).Magnitude
								if away > farthest then
									far, farthest = spot, away
								end
							end
							hidingAt[c.model] = far
						end
						Arena.go(c, hidingAt[c.model])
					end
				end
			end
		end
		return left() == 0
	end, 0.1)

	Arena.clock(nil)
	local everyone = Arena.everyone()
	if allFound then
		Arena.status("Seekers win")
		Arena.tell("Seekers win")
		for _, c in everyone do
			Arena.reset(c)
			if c.model == people[1].model then
				Arena.bump(c, "Wins")
				Arena.role(c, "Winner")
			end
		end
	else
		local names = {}
		for _, c in everyone do
			local stillHidden = not seekers[c.model]
			Arena.reset(c)
			if stillHidden and c.model:IsDescendantOf(workspace) then
				table.insert(names, c.name)
				Arena.bump(c, "Wins")
				Arena.role(c, "Winner")
			end
		end
		Arena.status("Hiders win")
		Arena.tell("Never found: " .. table.concat(names, ", "))
	end
end

while true do
	Arena.status("Next round")
	Arena.countdown(BREAK)
	Arena.clock(nil)
	playRound()
	task.wait(5)
end
