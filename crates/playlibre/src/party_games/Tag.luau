-- Tag. One player is It and glows red. Touch someone to make them It;
-- they can't tag you straight back. When the clock runs out, whoever
-- spent the least time being It wins the round. Bots join in when there
-- aren't enough people.

local Players = game:GetService("Players")
local Arena = require(script.Parent:WaitForChild("Arena"))

local PLAYERS = 6 -- bots fill up to this many
local ROUND = 90
local BREAK = 8
local REACH = 4.5 -- how close a tag is
local TAG_BACK = 2 -- seconds before the new It can tag
local NO_TAG_BACKS = 4 -- and a few more before they can tag whoever got them
local RUN_SPEED, IT_SPEED = 16, 18
local BOT_RUN_SPEED, BOT_IT_SPEED = 14, 16.5
local RED = Color3.fromRGB(235, 50, 45)

local floor = workspace:WaitForChild("Arena"):WaitForChild("Floor")
local spawns = Arena.spots(workspace:WaitForChild("Spawns"))
local halfX, halfZ = floor.Size.X / 2 - 6, floor.Size.Z / 2 - 6

Arena.score("Wins")
Arena.score("Tags")

local function inside(position)
	return Vector3.new(
		math.clamp(position.X, floor.Position.X - halfX, floor.Position.X + halfX),
		floor.Position.Y + floor.Size.Y / 2,
		math.clamp(position.Z, floor.Position.Z - halfZ, floor.Position.Z + halfZ)
	)
end

local function randomSpot()
	return inside(floor.Position + Vector3.new(math.random(-200, 200), 0, math.random(-200, 200)))
end

------------------------------------------------------------------------
-- A round.

local function playRound()
	Arena.fillWithBots(PLAYERS, spawns[1])
	task.wait(0.5)
	local people = Arena.everyone()
	if #people < 2 then
		return
	end
	Arena.shuffle(spawns)
	for i, c in people do
		Arena.reset(c)
		Arena.place(c, spawns[(i - 1) % #spawns + 1])
		Arena.role(c, "Runner")
		Arena.speed(c, c.bot and BOT_RUN_SPEED or RUN_SPEED)
	end

	local itTime = {} -- seconds each contestant has been It, by model
	local it
	local taggedBy -- the last one to tag It, who can't be tagged straight back
	local canTagAt = 0
	local function makeIt(c, by)
		if it then
			Arena.mark(it, nil)
			Arena.role(it, "Runner")
			Arena.speed(it, it.bot and BOT_RUN_SPEED or RUN_SPEED)
		end
		it = c
		taggedBy = by and by.model
		Arena.mark(c, RED)
		Arena.role(c, "It")
		-- A moment's pause so whoever tagged them can get away.
		Arena.speed(c, 0)
		canTagAt = time() + TAG_BACK
		task.delay(TAG_BACK * 0.6, function()
			if it and it.model == c.model and Arena.inGame(c) then
				Arena.speed(c, c.bot and BOT_IT_SPEED or IT_SPEED)
			end
		end)
		if by then
			Arena.bump(by, "Tags")
			Arena.tell(by.name .. " tagged " .. c.name)
		else
			Arena.tell(c.name .. " is It")
		end
		Arena.status(c.name .. " is It")
	end
	makeIt(people[math.random(#people)])

	local finish = time() + ROUND
	local nextThink = 0
	local last = time()
	while time() < finish do
		task.wait(0.05)
		local now = time()
		local dt = now - last
		last = now
		Arena.clock(finish - now)

		-- Anyone who arrived mid-round joins in as a runner.
		local everyone = Arena.everyone()
		for _, c in everyone do
			if c.model:GetAttribute("Role") ~= "Runner" and c.model:GetAttribute("Role") ~= "It" then
				Arena.place(c, randomSpot())
				Arena.role(c, "Runner")
				Arena.speed(c, c.bot and BOT_RUN_SPEED or RUN_SPEED)
			end
		end
		people = everyone
		if #people < 2 then
			break
		end

		-- It left: someone else takes over.
		if not it or not Arena.inGame(it) then
			it = nil
			makeIt(people[math.random(#people)])
		end
		for _, c in people do
			if c.model == it.model then
				it = c
			end
		end
		itTime[it.model] = (itTime[it.model] or 0) + dt

		-- Tags.
		if now >= canTagAt then
			for _, c in people do
				-- No tag-backs: whoever just tagged It is safe from them a while longer.
				local tagBack = c.model == taggedBy and now < canTagAt + NO_TAG_BACKS
				if c.model ~= it.model and not tagBack then
					local across, up = Arena.distance(it, c)
					if across < REACH and up < 5 then
						makeIt(c, it)
						break
					end
				end
			end
		end

		-- Bots think a few times a second.
		if now >= nextThink then
			nextThink = now + 0.25
			for _, c in people do
				if c.bot then
					if c.model == it.model then
						-- Chase the nearest runner.
						local best, bestDistance = nil, math.huge
						for _, other in people do
							if other.model ~= c.model then
								local d = Arena.distance(c, other)
								if other.model == taggedBy then
									d += 25
								end
								if d < bestDistance then
									best, bestDistance = other, d
								end
							end
						end
						if best then
							Arena.walk(c, best.root.Position)
						end
					else
						local d = Arena.distance(c, it)
						if d < 30 then
							-- Run away, a little to the side so corners don't trap them.
							local away = (c.root.Position - it.root.Position) * Vector3.new(1, 0, 1)
							if away.Magnitude < 0.1 then
								away = Vector3.new(1, 0, 0)
							end
							away = away.Unit
							local side = Vector3.new(-away.Z, 0, away.X) * (c.model.Name:len() % 2 == 0 and 1 or -1)
							-- Near a wall, they cut back towards the middle.
							local centre = (floor.Position - c.root.Position) * Vector3.new(1, 0, 1)
							local edge = math.max(math.abs(centre.X) / halfX, math.abs(centre.Z) / halfZ)
							local pull = edge > 0.6 and centre.Unit * 14 * edge or Vector3.zero
							local target = inside(c.root.Position + away * 18 + side * 8 + pull)
							if (target - c.root.Position).Magnitude < 8 then
								target = inside(c.root.Position + side * 20 - away * 4)
							end
							Arena.walk(c, target)
						elseif math.random() < 0.08 then
							Arena.go(c, randomSpot())
						end
					end
				end
			end
		end
	end

	-- The round's over: the one who was It least wins.
	Arena.clock(nil)
	local winner, least = nil, math.huge
	for _, c in Arena.everyone() do
		local t = itTime[c.model] or 0
		if t < least then
			winner, least = c, t
		end
	end
	for _, c in Arena.everyone() do
		Arena.reset(c)
		Arena.walk(c, c.root.Position)
	end
	if winner then
		Arena.bump(winner, "Wins")
		Arena.role(winner, "Winner")
		Arena.status(winner.name .. " wins")
		Arena.tell(winner.name .. " wins (" .. math.floor(least) .. "s as It)")
	end
end

while true do
	Arena.status("Next round")
	Arena.countdown(BREAK)
	Arena.clock(nil)
	playRound()
	task.wait(4)
end
