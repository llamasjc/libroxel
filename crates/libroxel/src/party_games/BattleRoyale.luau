-- Battle royale, lite. Everyone lands on the island with a sword. Crates
-- hold blasters and medkits. The storm closes in: outside the blue ring
-- you lose health, and it shrinks every stage. Last one standing wins.

local Players = game:GetService("Players")
local ServerStorage = game:GetService("ServerStorage")
local Debris = game:GetService("Debris")
local Arena = require(script.Parent:WaitForChild("Arena"))

local PLAYERS = 8
local BREAK = 10
local SWORD_DAMAGE, SWORD_REACH, SWORD_RATE = 25, 7, 0.45
local BLASTER_DAMAGE, BLASTER_RANGE, BLASTER_RATE, BLASTER_AIM = 18, 120, 0.35, 0.94
local BOT_DAMAGE, BOT_REACH, BOT_RATE = 8, 5, 1.1
local BOT_SIGHT = 32 -- how far away bots notice someone to fight
local PEACE = 15 -- seconds at the start to grab loot before anyone can fight
local BOT_SHOT_DAMAGE, BOT_SHOT_RANGE, BOT_SHOT_RATE = 8, 60, 1.2
local MEDKIT = 40
-- The storm: each stage waits, then shrinks the ring to this radius.
local STAGES = {
	{ wait = 25, shrink = 15, radius = 95, hurt = 3 },
	{ wait = 20, shrink = 15, radius = 60, hurt = 5 },
	{ wait = 15, shrink = 15, radius = 32, hurt = 8 },
	{ wait = 15, shrink = 15, radius = 12, hurt = 12 },
	{ wait = 10, shrink = 20, radius = 0, hurt = 20 },
}

local island = workspace:WaitForChild("Island")
local ground = island:WaitForChild("Ground")
local zone = workspace:WaitForChild("Storm")
local loot = workspace:WaitForChild("Loot")
local lobby = workspace:WaitForChild("Lobby"):WaitForChild("LobbyFloor")
local drops = Arena.spots(workspace:WaitForChild("Drops"))
local lobbySpot = lobby.Position + Vector3.new(0, lobby.Size.Y / 2, 0)

Arena.score("Wins")
Arena.score("KOs")

local START_RADIUS = ground.Size.X / 2 + 10
local zoneCentre, zoneRadius = ground.Position, START_RADIUS

local function drawZone()
	-- A tall see-through cylinder, stood on its end.
	local height = 120
	zone.Size = Vector3.new(height, zoneRadius * 2, zoneRadius * 2)
	zone.CFrame = CFrame.new(zoneCentre.X, ground.Position.Y + height / 2 - 2, zoneCentre.Z)
		* CFrame.Angles(0, 0, math.rad(90))
	zone.Transparency = zoneRadius > 0.5 and 0.75 or 1
end

local function outside(c)
	local offset = (c.root.Position - zoneCentre) * Vector3.new(1, 0, 1)
	return offset.Magnitude > zoneRadius
end

------------------------------------------------------------------------
-- Who's still in.

local fighters = {} -- by model: the contestant
local lastHit = {} -- by model: who hit them last
local cooldown = {} -- by model: when they can hit again
local roundOn = false
local peaceUntil = 0

local function standing()
	local list = {}
	for model, c in fighters do
		if Arena.inGame(c) then
			table.insert(list, c)
		else
			fighters[model] = nil
		end
	end
	return list
end

local function hurt(victim, amount, by)
	if victim.humanoid.Health <= 0 then
		return
	end
	lastHit[victim.model] = by
	victim.humanoid:TakeDamage(amount)
	if victim.humanoid.Health <= 0 then
		fighters[victim.model] = nil
		local killer = lastHit[victim.model]
		Arena.role(victim, "Out")
		if killer and killer.model ~= victim.model then
			Arena.bump(killer, "KOs")
			Arena.tell(killer.name .. " knocked out " .. victim.name)
		else
			Arena.tell(victim.name .. " was caught in the storm")
		end
		if victim.bot then
			Debris:AddItem(victim.model, 2)
		end
	end
end

local function zap(from, to)
	local beam = Instance.new("Part")
	beam.Name = "Zap"
	beam.Anchored = true
	beam.CanCollide = false
	beam.Material = Enum.Material.Neon
	beam.Color = Color3.fromRGB(90, 230, 255)
	local length = (to - from).Magnitude
	beam.Size = Vector3.new(0.25, 0.25, length)
	beam.CFrame = CFrame.lookAt((from + to) / 2, to)
	beam.Parent = workspace
	Debris:AddItem(beam, 0.12)
end

-- The nearest rival in front of `c`, within `range` and roughly where
-- they're facing (`aim` is how lined up, as a dot product).
local function target(c, range, aim)
	local look = c.root.CFrame.LookVector * Vector3.new(1, 0, 1)
	look = look.Magnitude > 0 and look.Unit or Vector3.new(0, 0, -1)
	local best, bestDistance = nil, range
	for _, other in standing() do
		if other.model ~= c.model then
			local offset = (other.root.Position - c.root.Position) * Vector3.new(1, 0, 1)
			local d = offset.Magnitude
			local lined = d < 2 and 1 or offset.Unit:Dot(look)
			if d < bestDistance and lined >= aim and Arena.sees(c, other) then
				best, bestDistance = other, d
			end
		end
	end
	return best
end

------------------------------------------------------------------------
-- Weapons. The server gives them out and decides every hit.

local function ready(c, rate)
	local now = time()
	if (cooldown[c.model] or 0) > now then
		return false
	end
	cooldown[c.model] = now + rate
	return true
end

local function contestantFor(tool)
	local model = tool.Parent
	return model and fighters[model]
end

local function give(player, name)
	local template = ServerStorage:FindFirstChild(name)
	local backpack = player:FindFirstChildOfClass("Backpack")
	if not template or not backpack then
		return
	end
	if backpack:FindFirstChild(name) or (player.Character and player.Character:FindFirstChild(name)) then
		return
	end
	local tool = template:Clone()
	tool.Parent = backpack
	tool.Activated:Connect(function()
		local c = contestantFor(tool)
		if not c or not roundOn or time() < peaceUntil then
			return
		end
		if name == "Sword" then
			if ready(c, SWORD_RATE) then
				local victim = target(c, SWORD_REACH, 0.3)
				if victim then
					hurt(victim, SWORD_DAMAGE, c)
				end
			end
		elseif name == "Blaster" then
			if ready(c, BLASTER_RATE) then
				local from = c.root.Position + Vector3.new(0, 1.5, 0)
				local victim = target(c, BLASTER_RANGE, BLASTER_AIM)
				if victim then
					zap(from, victim.root.Position + Vector3.new(0, 1.5, 0))
					hurt(victim, BLASTER_DAMAGE, c)
				else
					zap(from, from + c.root.CFrame.LookVector * 40)
				end
			end
		end
	end)
end

local function takeWeapons(player)
	for _, place in { player:FindFirstChildOfClass("Backpack"), player.Character } do
		if place then
			for _, item in place:GetChildren() do
				if item:IsA("Tool") then
					item:Destroy()
				end
			end
		end
	end
end

------------------------------------------------------------------------
-- Crates.

local hasBlaster = {} -- bots, by model

local function openCrates()
	for _, crate in loot:GetChildren() do
		if crate:IsA("BasePart") and crate:GetAttribute("Open") ~= true then
			for _, c in standing() do
				if (c.root.Position - crate.Position).Magnitude < 5 then
					crate:SetAttribute("Open", true)
					crate.Transparency = 0.8
					local blaster = (c.player and not hasBlaster[c.model] and math.random() < 0.6)
						or (c.bot and not hasBlaster[c.model] and math.random() < 0.5)
					if blaster then
						hasBlaster[c.model] = true
						if c.player then
							give(c.player, "Blaster")
							Arena.tell("You found a blaster!", c.player)
						end
					else
						c.humanoid.Health = math.min(c.humanoid.MaxHealth, c.humanoid.Health + MEDKIT)
						if c.player then
							Arena.tell("Medkit: +" .. MEDKIT .. " health", c.player)
						end
					end
					break
				end
			end
		end
	end
end

local function resetCrates()
	for _, crate in loot:GetChildren() do
		if crate:IsA("BasePart") then
			crate:SetAttribute("Open", false)
			crate.Transparency = 0
		end
	end
end

------------------------------------------------------------------------
-- Bots.

local wander = {}

local function randomInZone()
	local angle = math.random() * math.pi * 2
	local r = math.sqrt(math.random()) * math.max(zoneRadius - 8, 2)
	return zoneCentre + Vector3.new(math.cos(angle) * r, 0, math.sin(angle) * r)
end

local function think(c, list)
	if outside(c) then
		Arena.go(c, zoneCentre)
		return
	end
	local nearest, d = nil, math.huge
	for _, other in list do
		if other.model ~= c.model then
			local distance = Arena.distance(c, other)
			if distance < d then
				nearest, d = other, distance
			end
		end
	end
	if nearest and d < BOT_SIGHT and time() >= peaceUntil then
		-- Straight at them if nothing's in the way, else round whatever is.
		if Arena.sees(c, nearest) then
			Arena.walk(c, nearest.root.Position)
		else
			Arena.go(c, nearest.root.Position)
		end
		if d < BOT_REACH and ready(c, BOT_RATE) then
			hurt(nearest, BOT_DAMAGE, c)
		elseif hasBlaster[c.model] and d < BOT_SHOT_RANGE and Arena.sees(c, nearest) and ready(c, BOT_SHOT_RATE) then
			local from = c.root.Position + Vector3.new(0, 1.5, 0)
			zap(from, nearest.root.Position + Vector3.new(0, 1.5, 0))
			if math.random() < 0.6 then
				hurt(nearest, BOT_SHOT_DAMAGE, c)
			end
		end
		return
	end
	-- Nobody near: look for a crate, or wander inside the ring.
	local goal = wander[c.model]
	if not goal or (c.root.Position - goal).Magnitude < 4 or math.random() < 0.02 then
		goal = randomInZone()
		for _, crate in loot:GetChildren() do
			if crate:IsA("BasePart") and crate:GetAttribute("Open") ~= true then
				local inRing = ((crate.Position - zoneCentre) * Vector3.new(1, 0, 1)).Magnitude < zoneRadius - 4
				if inRing and (crate.Position - c.root.Position).Magnitude < 40 then
					goal = crate.Position
					break
				end
			end
		end
		wander[c.model] = goal
	end
	Arena.go(c, goal)
end

------------------------------------------------------------------------
-- People who die or arrive mid-round wait in the lobby.

local function toLobby(character)
	task.wait()
	local root = character:WaitForChild("HumanoidRootPart", 5)
	if root and roundOn and not fighters[character] then
		character:PivotTo(CFrame.new(lobbySpot + Vector3.new(math.random(-8, 8), 3, math.random(-8, 8))))
		local player = Players:GetPlayerFromCharacter(character)
		if player then
			player:SetAttribute("Role", "Out")
		end
	end
end

local function watch(player)
	player.CharacterAdded:Connect(toLobby)
end
Players.PlayerAdded:Connect(watch)
for _, player in Players:GetPlayers() do
	watch(player)
end

------------------------------------------------------------------------
-- A round.

local function playRound()
	Arena.clearDeadBots()
	Arena.fillWithBots(PLAYERS, lobbySpot)
	task.wait(0.5)
	local people = Arena.everyone()
	if #people < 2 then
		return
	end
	zoneCentre, zoneRadius = ground.Position, START_RADIUS
	drawZone()
	resetCrates()
	table.clear(fighters)
	table.clear(lastHit)
	table.clear(hasBlaster)
	table.clear(wander)
	Arena.shuffle(drops)
	for i, c in people do
		Arena.reset(c)
		c.humanoid.Health = c.humanoid.MaxHealth
		fighters[c.model] = c
		Arena.role(c, "Fighter")
		Arena.place(c, drops[(i - 1) % #drops + 1])
		if c.player then
			takeWeapons(c.player)
			give(c.player, "Sword")
		end
	end
	roundOn = true
	peaceUntil = time() + PEACE
	Arena.tell("Grab loot! Fighting starts in " .. PEACE .. " seconds")
	task.delay(PEACE, function()
		if roundOn then
			Arena.tell("Fight! Stay inside the ring")
		end
	end)

	local stage, stageStart, shrinking = 1, time(), false
	local fromCentre, fromRadius, toCentre = zoneCentre, zoneRadius, zoneCentre
	local lastHurt = time()
	local nextThink = 0
	while true do
		task.wait(0.1)
		local now = time()
		local list = standing()
		if #list <= 1 then
			break
		end
		if now < peaceUntil then
			Arena.status("Grab loot! Fighting in " .. math.ceil(peaceUntil - now))
		else
			Arena.status(#list .. " left")
		end

		-- The storm.
		local s = STAGES[math.min(stage, #STAGES)]
		if not shrinking then
			Arena.clock(stageStart + s.wait - now)
			if now >= stageStart + s.wait then
				shrinking, stageStart = true, now
				fromCentre, fromRadius = zoneCentre, zoneRadius
				-- The new ring sits somewhere inside the old one.
				local room = math.max(fromRadius - s.radius, 0)
				local angle = math.random() * math.pi * 2
				local r = math.random() * room * 0.6
				toCentre = fromCentre + Vector3.new(math.cos(angle) * r, 0, math.sin(angle) * r)
				Arena.tell("The storm is closing in!")
			end
		else
			local t = math.clamp((now - stageStart) / s.shrink, 0, 1)
			Arena.clock(stageStart + s.shrink - now)
			zoneCentre = fromCentre:Lerp(toCentre, t)
			zoneRadius = fromRadius + (s.radius - fromRadius) * t
			drawZone()
			if t >= 1 then
				shrinking, stageStart = false, now
				stage += 1
			end
		end
		if now - lastHurt >= 1 then
			lastHurt = now
			for _, c in list do
				if outside(c) then
					hurt(c, s.hurt, nil)
				end
			end
		end

		openCrates()
		if now >= nextThink then
			nextThink = now + 0.25
			for _, c in standing() do
				if c.bot then
					think(c, list)
				end
			end
		end
	end

	roundOn = false
	Arena.clock(nil)
	local winner = standing()[1]
	if winner then
		Arena.bump(winner, "Wins")
		Arena.role(winner, "Winner")
		Arena.status(winner.name .. " wins!")
		Arena.tell(winner.name .. " is the last one standing!")
	else
		Arena.status("Nobody made it!")
	end
	task.wait(5)
	-- Everyone back to the lobby for the next round.
	for _, c in Arena.everyone() do
		Arena.reset(c)
		if c.player then
			takeWeapons(c.player)
		end
		Arena.place(c, lobbySpot + Vector3.new(math.random(-8, 8), 0, math.random(-8, 8)))
	end
end

drawZone()
while true do
	Arena.status("Battle royale starts soon")
	Arena.countdown(BREAK)
	Arena.clock(nil)
	playRound()
end
