-- Battle royale. Everyone rides the airship over the island and jumps out
-- where they like, gliding down. Chests hold weapons in five tiers, medkits
-- and shields. Chop trees and break rocks for wood, and build walls, floors
-- and ramps with it. The storm closes in, phase by phase. Last one standing
-- wins.

local Players = game:GetService("Players")
local ServerStorage = game:GetService("ServerStorage")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")
local Debris = game:GetService("Debris")
local Arena = require(script.Parent:WaitForChild("Arena"))

local PLAYERS = 8
local BREAK = 10
local MAX_SHIELD = 100
local MEDKIT, SHIELD = 50, 50
local CHEST_WOOD = 30
local CHOP, CHOPS_PER_TREE = 10, 6
local MAX_WOOD = 500
local GRID, COST = 8, 10
local MAX_BUILDS = 250
local BUILD_HEALTH = { Wall = 150, Floor = 140, Ramp = 140 }
local SWORD_DAMAGE, SWORD_REACH, SWORD_RATE, SWORD_BREAK = 20, 7, 0.45, 50
local BOT_DAMAGE, BOT_REACH, BOT_RATE = 8, 5, 1.1
local BOT_SIGHT = 40
local BOT_WALL_RATE = 6
local SHIP_HEIGHT, SHIP_TIME, SHIP_REACH = 110, 22, 170
local GLIDE_FALL, GLIDE_SPEED, GLIDE_LONGEST = 22, 26, 25

local TIERS = {
	{ name = "Common", colour = Color3.fromRGB(190, 190, 195), weight = 40 },
	{ name = "Uncommon", colour = Color3.fromRGB(80, 200, 90), weight = 28 },
	{ name = "Rare", colour = Color3.fromRGB(70, 140, 240), weight = 18 },
	{ name = "Epic", colour = Color3.fromRGB(170, 90, 230), weight = 10 },
	{ name = "Legendary", colour = Color3.fromRGB(245, 170, 40), weight = 4 },
}
local WEAPONS = {
	Blaster = { damage = { 14, 16, 18, 20, 23 }, range = 120, rate = 0.3, spread = 3, bot = 60 },
	Scattergun = { damage = { 45, 50, 56, 62, 70 }, range = 30, rate = 0.9, spread = 5, bot = 22 },
	Longshot = { damage = { 60, 68, 76, 85, 95 }, range = 260, rate = 1.6, spread = 1.5, bot = 110 },
}
local WEAPON_NAMES = { "Blaster", "Scattergun", "Longshot" }
local STAGES = {
	{ wait = 30, shrink = 15, radius = 95, hurt = 2 },
	{ wait = 25, shrink = 15, radius = 65, hurt = 4 },
	{ wait = 20, shrink = 15, radius = 40, hurt = 6 },
	{ wait = 15, shrink = 15, radius = 22, hurt = 9 },
	{ wait = 12, shrink = 12, radius = 10, hurt = 12 },
	{ wait = 10, shrink = 15, radius = 0, hurt = 20 },
}

local island = workspace:WaitForChild("Island")
local ground = island:WaitForChild("Ground")
local zone = workspace:WaitForChild("Storm")
local nextRing = workspace:WaitForChild("NextRing")
local loot = workspace:WaitForChild("Loot")
local airship = workspace:WaitForChild("Airship")
local hull = airship:WaitForChild("Hull")
local lobby = workspace:WaitForChild("Lobby"):WaitForChild("LobbyFloor")
local lobbySpot = lobby.Position + Vector3.new(0, lobby.Size.Y / 2, 0)
local GROUND_Y = ground.Position.Y + ground.Size.Y / 2

for _, part in island:GetDescendants() do
	if part:IsA("BasePart") and (part.Name == "Trunk" or part.Name == "Leaves") then
		part:SetAttribute("Tree", math.floor(part.Position.X + 0.5) .. "," .. math.floor(part.Position.Z + 0.5))
	end
end

local builds = Instance.new("Folder")
builds.Name = "Builds"
builds.Parent = workspace

local remotes = Instance.new("Folder")
remotes.Name = "Royale"
remotes.Parent = ReplicatedStorage
local function remote(name)
	local event = Instance.new("RemoteEvent")
	event.Name = name
	event.Parent = remotes
	return event
end
local dropEvent = remote("Drop")
local buildEvent = remote("Build")
local resultEvent = remote("Result")

Arena.score("Wins")
Arena.score("KOs")

local START_RADIUS = ground.Size.X / 2 + 10
local zoneCentre, zoneRadius = ground.Position, START_RADIUS

local function drawZone()
	local height = 160
	zone.Size = Vector3.new(height, zoneRadius * 2, zoneRadius * 2)
	zone.CFrame = CFrame.new(zoneCentre.X, ground.Position.Y + height / 2 - 2, zoneCentre.Z)
		* CFrame.Angles(0, 0, math.rad(90))
	zone.Transparency = zoneRadius > 0.5 and 0.75 or 1
end

local function drawNext(centre, radius)
	if not centre or radius < 1 then
		nextRing.Transparency = 1
		return
	end
	nextRing.Size = Vector3.new(0.3, radius * 2, radius * 2)
	nextRing.CFrame = CFrame.new(centre.X, GROUND_Y + 0.2, centre.Z) * CFrame.Angles(0, 0, math.rad(90))
	nextRing.Transparency = 0.7
end

local function outside(c)
	local offset = (c.root.Position - zoneCentre) * Vector3.new(1, 0, 1)
	return offset.Magnitude > zoneRadius
end

------------------------------------------------------------------------

local fighters = {}
local kits = {}
local lastHit = {}
local cooldown = {}
local roundOn = false
local total = 0

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

local function kitOf(c)
	local kit = kits[c.model]
	if not kit then
		kit = { wood = 0, shield = 0, weapons = {} }
		kits[c.model] = kit
	end
	return kit
end

local function show(c, name, value)
	c.model:SetAttribute(name, value)
	if c.player then
		c.player:SetAttribute(name, value)
	end
end

local function setShield(c, value)
	kitOf(c).shield = value
	show(c, "Shield", value)
end

local function setWood(c, value)
	kitOf(c).wood = math.clamp(value, 0, MAX_WOOD)
	show(c, "Wood", kitOf(c).wood)
end

local function role(c)
	return c.model:GetAttribute("Role")
end

local function landed(c)
	local r = role(c)
	return r == "Fighter" or r == "Winner"
end

local wallBack

local function hurt(victim, amount, by)
	if victim.humanoid.Health <= 0 or not fighters[victim.model] then
		return
	end
	lastHit[victim.model] = by
	local kit = kitOf(victim)
	local soak = math.min(kit.shield, amount)
	if soak > 0 then
		setShield(victim, kit.shield - soak)
		amount -= soak
	end
	if amount > 0 then
		victim.humanoid:TakeDamage(amount)
	end
	if victim.humanoid.Health > 0 then
		if by and victim.bot then
			wallBack(victim, by)
		end
		return
	end
	fighters[victim.model] = nil
	local place = #standing() + 1
	local killer = lastHit[victim.model]
	Arena.role(victim, "Out")
	if killer and killer.model ~= victim.model then
		Arena.bump(killer, "KOs")
		show(killer, "KOs", (killer.model:GetAttribute("KOs") or 0) + 1)
		setWood(killer, kitOf(killer).wood + kit.wood)
		Arena.tell(killer.name .. " knocked out " .. victim.name)
	else
		Arena.tell(victim.name .. " was caught in the storm")
	end
	if victim.player then
		resultEvent:FireClient(victim.player, place, total, killer and killer.model ~= victim.model and killer.name or nil)
	end
	if victim.bot then
		Debris:AddItem(victim.model, 2)
	end
end

local function zap(from, to, colour)
	local beam = Instance.new("Part")
	beam.Name = "Zap"
	beam.Anchored = true
	beam.CanCollide = false
	beam.CanQuery = false
	beam.Material = Enum.Material.Neon
	beam.Color = colour or Color3.fromRGB(90, 230, 255)
	local length = (to - from).Magnitude
	beam.Size = Vector3.new(0.25, 0.25, length)
	beam.CFrame = CFrame.lookAt((from + to) / 2, to)
	beam.Parent = workspace
	Debris:AddItem(beam, 0.12)
end

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
-- Building.

local function hitBuild(part, amount)
	if part.Parent ~= builds then
		return false
	end
	local health = (part:GetAttribute("Health") or 0) - amount
	part:SetAttribute("Health", health)
	if health <= 0 then
		part:Destroy()
	else
		part.Transparency = 0.35 * (1 - health / (BUILD_HEALTH[part.Name] or 150))
	end
	return true
end

local function cell(v)
	return math.floor(v / GRID) * GRID + GRID / 2
end

local function sign(v)
	return v < 0 and -1 or 1
end

local function taken(centre, kind)
	for _, piece in builds:GetChildren() do
		if piece.Name == kind and (piece.Position - centre).Magnitude < 1 then
			return true
		end
	end
	return false
end

local function build(c, kind, look)
	if not BUILD_HEALTH[kind] or not landed(c) or not roundOn then
		return nil
	end
	local kit = kitOf(c)
	if kit.wood < COST or #builds:GetChildren() >= MAX_BUILDS then
		return nil
	end
	local flat = (typeof(look) == "Vector3" and look or c.root.CFrame.LookVector) * Vector3.new(1, 0, 1)
	if flat.Magnitude < 0.01 then
		flat = c.root.CFrame.LookVector * Vector3.new(1, 0, 1)
	end
	local dir = math.abs(flat.X) > math.abs(flat.Z) and Vector3.new(sign(flat.X), 0, 0)
		or Vector3.new(0, 0, sign(flat.Z))
	local pos = c.root.Position
	local feet = pos.Y - 3 - GROUND_Y
	local level = GROUND_Y + math.max(0, math.floor((feet + 1) / GRID)) * GRID
	local here = Vector3.new(cell(pos.X), level, cell(pos.Z))
	local part
	if kind == "Wall" then
		local centre = here + dir * (GRID / 2) + Vector3.new(0, GRID / 2, 0)
		if taken(centre, kind) then
			return nil
		end
		part = Instance.new("Part")
		part.Size = dir.X ~= 0 and Vector3.new(0.8, GRID, GRID) or Vector3.new(GRID, GRID, 0.8)
		part.CFrame = CFrame.new(centre)
	elseif kind == "Floor" then
		local centre = here + dir * GRID + Vector3.new(0, -0.25, 0)
		if taken(centre, kind) then
			return nil
		end
		part = Instance.new("Part")
		part.Size = Vector3.new(GRID, 0.5, GRID)
		part.CFrame = CFrame.new(centre)
	else
		local centre = here + dir * GRID + Vector3.new(0, GRID / 2, 0)
		if taken(centre, kind) then
			return nil
		end
		part = Instance.new("WedgePart")
		part.Size = Vector3.new(GRID, GRID, GRID)
		part.CFrame = CFrame.lookAt(centre, centre - dir)
	end
	part.Name = kind
	part.Anchored = true
	part.Material = Enum.Material.WoodPlanks
	part.Color = Color3.fromRGB(190, 145, 95)
	part:SetAttribute("Health", BUILD_HEALTH[kind])
	part.Parent = builds
	setWood(c, kit.wood - COST)
	return part
end

buildEvent.OnServerEvent:Connect(function(player, kind, look)
	local c = player.Character and fighters[player.Character]
	if c and typeof(kind) == "string" then
		build(c, kind, look)
	end
end)

local lastWall = {}
function wallBack(c, from)
	if (lastWall[c.model] or 0) > time() or kitOf(c).wood < COST or not landed(c) then
		return
	end
	lastWall[c.model] = time() + BOT_WALL_RATE
	build(c, "Wall", from.root.Position - c.root.Position)
end

------------------------------------------------------------------------
-- Weapons.

local function ready(c, rate)
	local now = time()
	if (cooldown[c.model] or 0) > now then
		return false
	end
	cooldown[c.model] = now + rate
	return true
end

local aims = {}

local aimEvent = Instance.new("RemoteEvent")
aimEvent.Name = "Aim"
aimEvent.Parent = ReplicatedStorage
aimEvent.OnServerEvent:Connect(function(player, at)
	if typeof(at) == "Vector3" then
		aims[player] = at
	end
end)
Players.PlayerRemoving:Connect(function(player)
	aims[player] = nil
end)

local function shoot(c, weapon, tier, direction)
	local stats = WEAPONS[weapon]
	local from = c.root.Position + Vector3.new(0, 1.5, 0)
	if not direction then
		local at = c.player and aims[c.player]
		direction = at and (at - from).Magnitude > 1 and (at - from).Unit or c.root.CFrame.LookVector
	end
	local params = RaycastParams.new()
	params.FilterType = Enum.RaycastFilterType.Exclude
	params.FilterDescendantsInstances = { c.model, zone, nextRing }
	local hit = workspace:Raycast(from, direction * stats.range, params)
	local finish = hit and hit.Position or from + direction * stats.range
	local victim = nil
	if hit then
		if hitBuild(hit.Instance, stats.damage[tier]) then
			zap(from, finish, TIERS[tier].colour)
			return
		end
		local owner = hit.Instance:FindFirstAncestorOfClass("Model")
		victim = owner and fighters[owner]
	end
	if not victim then
		local reach, best = (finish - from).Magnitude, stats.spread
		for _, other in standing() do
			if other.model ~= c.model then
				local offset = other.root.Position - from
				local along = offset:Dot(direction)
				local off = (offset - direction * along).Magnitude
				if along > 0 and along <= reach + 2 and off < best then
					victim, best = other, off
				end
			end
		end
	end
	if victim then
		finish = victim.root.Position + Vector3.new(0, 1, 0)
		hurt(victim, stats.damage[tier], c)
	end
	zap(from, finish, TIERS[tier].colour)
end

local HARVEST = { Trunk = true, Leaves = true, Rock = true, Wall = true, Ruin = true }

local function swing(c)
	local victim = target(c, SWORD_REACH, 0.3)
	if victim then
		hurt(victim, SWORD_DAMAGE, c)
		return
	end
	local params = RaycastParams.new()
	params.FilterType = Enum.RaycastFilterType.Exclude
	params.FilterDescendantsInstances = { c.model, zone, nextRing }
	local look = c.root.CFrame.LookVector * Vector3.new(1, 0, 1)
	local hit = workspace:Raycast(c.root.Position, look.Unit * SWORD_REACH, params)
	if not hit then
		return
	end
	local part = hit.Instance
	if hitBuild(part, SWORD_BREAK) then
		return
	end
	if HARVEST[part.Name] and part:IsDescendantOf(island) then
		setWood(c, kitOf(c).wood + CHOP)
		if part.Name == "Trunk" then
			local chops = (part:GetAttribute("Chops") or 0) + 1
			part:SetAttribute("Chops", chops)
			if chops >= CHOPS_PER_TREE then
				for _, piece in part.Parent:GetChildren() do
					if piece:GetAttribute("Tree") == part:GetAttribute("Tree") then
						piece.Transparency = 1
						piece.CanCollide = false
						piece.CanQuery = false
					end
				end
			end
		end
	end
end

local function contestantFor(tool)
	local model = tool.Parent
	return model and fighters[model]
end

local function give(player, name, tier)
	local template = ServerStorage:FindFirstChild(name)
	local backpack = player:FindFirstChildOfClass("Backpack")
	if not template or not backpack then
		return
	end
	local tool = backpack:FindFirstChild(name) or (player.Character and player.Character:FindFirstChild(name))
	if not tool then
		tool = template:Clone()
		tool.Parent = backpack
		tool.Activated:Connect(function()
			local c = contestantFor(tool)
			if not c or not roundOn or not landed(c) then
				return
			end
			if name == "Sword" then
				if ready(c, SWORD_RATE) then
					swing(c)
				end
			elseif ready(c, WEAPONS[name].rate) then
				shoot(c, name, tool:GetAttribute("Tier") or 1)
			end
		end)
	end
	if tier then
		tool:SetAttribute("Tier", tier)
		local handle = tool:FindFirstChild("Handle")
		if handle then
			handle.Color = TIERS[tier].colour
		end
	end
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

local function bestWeapon(c)
	local best, bestTier, bestScore = nil, 0, 0
	for name, tier in kitOf(c).weapons do
		local stats = WEAPONS[name]
		local score = stats.damage[tier] / stats.rate
		if score > bestScore then
			best, bestTier, bestScore = name, tier, score
		end
	end
	return best, bestTier
end

------------------------------------------------------------------------
-- Chests.

local function rollTier()
	local sum = 0
	for _, t in TIERS do
		sum += t.weight
	end
	local roll = math.random() * sum
	for i, t in TIERS do
		roll -= t.weight
		if roll <= 0 then
			return i
		end
	end
	return 1
end

local function openChest(chest, c)
	chest:SetAttribute("Open", true)
	chest.Transparency = 0.8
	local kit = kitOf(c)
	local name = WEAPON_NAMES[math.random(#WEAPON_NAMES)]
	local tier = rollTier()
	local got = {}
	if (kit.weapons[name] or 0) < tier then
		kit.weapons[name] = tier
		if c.player then
			give(c.player, name, tier)
		end
		table.insert(got, TIERS[tier].name .. " " .. name)
	end
	if math.random() < 0.5 or c.humanoid.Health >= c.humanoid.MaxHealth then
		setShield(c, math.min(MAX_SHIELD, kit.shield + SHIELD))
		table.insert(got, "+" .. SHIELD .. " shield")
	else
		c.humanoid.Health = math.min(c.humanoid.MaxHealth, c.humanoid.Health + MEDKIT)
		table.insert(got, "+" .. MEDKIT .. " HP")
	end
	setWood(c, kit.wood + CHEST_WOOD)
	if c.player then
		Arena.tell(table.concat(got, "  "), c.player)
	end
end

local function openChests()
	for _, chest in loot:GetChildren() do
		if chest:IsA("BasePart") and chest:GetAttribute("Open") ~= true then
			for _, c in standing() do
				if landed(c) and (c.root.Position - chest.Position).Magnitude < 5 then
					openChest(chest, c)
					break
				end
			end
		end
	end
end

local function resetIsland()
	for _, chest in loot:GetChildren() do
		if chest:IsA("BasePart") then
			chest:SetAttribute("Open", false)
			chest.Transparency = 0
		end
	end
	for _, part in island:GetDescendants() do
		if part:IsA("BasePart") and part:GetAttribute("Tree") then
			part:SetAttribute("Chops", 0)
			part.Transparency = 0
			part.CanCollide = true
			part.CanQuery = true
		end
	end
	builds:ClearAllChildren()
end

------------------------------------------------------------------------
-- The airship and the drop.

local ship = { from = Vector3.zero, to = Vector3.zero, start = 0, riders = {}, on = false }
local shipParts = {}
for _, part in airship:GetChildren() do
	if part:IsA("BasePart") then
		shipParts[part] = hull.CFrame:ToObjectSpace(part.CFrame)
	end
end

local function shipAt(now)
	local t = math.clamp((now - ship.start) / SHIP_TIME, 0, 1)
	return ship.from:Lerp(ship.to, t), t
end

local function seatFor(i)
	local row, side = (i - 1) // 2, (i - 1) % 2
	return Vector3.new(side == 0 and -3 or 3, hull.Size.Y / 2 + 3, -10 + row * 3)
end

local function moveShip(now)
	local at = shipAt(now)
	local facing = CFrame.lookAt(at, at + (ship.to - ship.from))
	for part, offset in shipParts do
		part.CFrame = facing * offset
	end
	for model, seat in ship.riders do
		local c = fighters[model]
		if c and Arena.inGame(c) then
			c.model:PivotTo(facing * CFrame.new(seatFor(seat)))
		end
	end
end

local function parkShip()
	ship.on = false
	for part, offset in shipParts do
		part.CFrame = CFrame.new(0, SHIP_HEIGHT, 260) * offset
	end
end

local glider = ServerStorage:WaitForChild("Glider")

local function jump(c)
	if not ship.riders[c.model] then
		return
	end
	ship.riders[c.model] = nil
	if not Arena.inGame(c) then
		return
	end
	local at = shipAt(time())
	c.root.Anchored = false
	c.model:PivotTo(CFrame.new(at - Vector3.new(0, 5, 0)) * c.root.CFrame.Rotation)
	c.root.AssemblyLinearVelocity = Vector3.new(0, -GLIDE_FALL, 0)
	Arena.role(c, "Gliding")
	c.model:SetAttribute("GlideUntil", time() + GLIDE_LONGEST)
	local wing = glider:Clone()
	wing.Name = "Glider"
	wing.CFrame = c.root.CFrame * CFrame.new(0, 4, 0)
	local weld = Instance.new("WeldConstraint")
	weld.Part0 = c.root
	weld.Part1 = wing
	weld.Parent = wing
	wing.Parent = c.model
end

local function land(c)
	local wing = c.model:FindFirstChild("Glider")
	if wing then
		wing:Destroy()
	end
	Arena.role(c, "Fighter")
end

dropEvent.OnServerEvent:Connect(function(player)
	local c = player.Character and fighters[player.Character]
	if c then
		jump(c)
	end
end)

local groundParams = RaycastParams.new()
groundParams.FilterType = Enum.RaycastFilterType.Exclude

local landingFor = {}
local jumpAt = {}

local function glide(c)
	local v = c.root.AssemblyLinearVelocity
	local fall = math.max(v.Y, -GLIDE_FALL)
	local goal = landingFor[c.model]
	if c.bot and goal then
		local flat = (goal - c.root.Position) * Vector3.new(1, 0, 1)
		local push = flat.Magnitude > 2 and flat.Unit * math.min(GLIDE_SPEED, flat.Magnitude * 2) or Vector3.zero
		c.root.AssemblyLinearVelocity = Vector3.new(push.X, fall, push.Z)
	elseif fall ~= v.Y then
		c.root.AssemblyLinearVelocity = Vector3.new(v.X, fall, v.Z)
	end
	groundParams.FilterDescendantsInstances = { c.model, zone, nextRing, airship }
	local below = workspace:Raycast(c.root.Position, Vector3.new(0, -4, 0), groundParams)
	local still = math.abs(v.Y) < 0.5 and time() > (c.model:GetAttribute("GlideUntil") or 0) - GLIDE_LONGEST + 1
	if below or still or c.root.Position.Y < GROUND_Y + 3.5 or time() > (c.model:GetAttribute("GlideUntil") or 0) then
		land(c)
	end
end

RunService.Heartbeat:Connect(function()
	if not roundOn then
		return
	end
	local now = time()
	if ship.on then
		moveShip(now)
		local _, t = shipAt(now)
		for model in ship.riders do
			local c = fighters[model]
			local spot = jumpAt[model]
			if c and c.bot and spot and ((shipAt(now) - spot) * Vector3.new(1, 0, 1)).Magnitude < 12 then
				jump(c)
			end
		end
		if t >= 1 then
			for model in ship.riders do
				if fighters[model] then
					jump(fighters[model])
				end
			end
			parkShip()
		end
	end
	for _, c in fighters do
		if role(c) == "Gliding" and Arena.inGame(c) then
			glide(c)
		end
	end
end)

local function launch(people)
	local angle = math.random() * math.pi * 2
	local across = Vector3.new(math.cos(angle), 0, math.sin(angle))
	local shift = Vector3.new(-across.Z, 0, across.X) * (math.random() * 60 - 30)
	local centre = ground.Position + shift + Vector3.new(0, SHIP_HEIGHT, 0)
	ship.from, ship.to = centre - across * SHIP_REACH, centre + across * SHIP_REACH
	ship.start = time()
	ship.on = true
	table.clear(ship.riders)
	table.clear(landingFor)
	table.clear(jumpAt)
	local chests = loot:GetChildren()
	for i, c in people do
		ship.riders[c.model] = i
		c.root.Anchored = true
		Arena.role(c, "Riding")
		if c.bot then
			local chest = chests[math.random(#chests)]
			local line = ship.to - ship.from
			local along = math.clamp((chest.Position - ship.from):Dot(line.Unit), 20, line.Magnitude - 20)
			local under = ship.from + line.Unit * along
			local flat = (chest.Position - under) * Vector3.new(1, 0, 1)
			local reach = math.min(flat.Magnitude, 70)
			jumpAt[c.model] = under
			landingFor[c.model] = under * Vector3.new(1, 0, 1)
				+ (flat.Magnitude > 0 and flat.Unit * reach or Vector3.zero)
				+ Vector3.new(0, GROUND_Y, 0)
		end
	end
	moveShip(time())
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
	if not landed(c) then
		return
	end
	if outside(c) then
		Arena.go(c, zoneCentre)
		return
	end
	local nearest, d = nil, math.huge
	for _, other in list do
		if other.model ~= c.model and landed(other) then
			local distance = Arena.distance(c, other)
			if distance < d then
				nearest, d = other, distance
			end
		end
	end
	local weapon, tier = bestWeapon(c)
	local reach = weapon and WEAPONS[weapon].bot or 0
	if nearest and d < math.max(BOT_SIGHT, reach) then
		if d > reach * 0.6 or not weapon then
			if Arena.sees(c, nearest) then
				Arena.walk(c, nearest.root.Position)
			else
				Arena.go(c, nearest.root.Position)
			end
		else
			Arena.walk(c, c.root.Position)
		end
		if d < BOT_REACH and ready(c, BOT_RATE) then
			hurt(nearest, BOT_DAMAGE, c)
		elseif weapon and d < reach and Arena.sees(c, nearest) and ready(c, WEAPONS[weapon].rate * 2.5) then
			local from = c.root.Position + Vector3.new(0, 1.5, 0)
			local miss = Vector3.new(math.random() - 0.5, math.random() - 0.5, math.random() - 0.5) * (d / 12)
			local aim = (nearest.root.Position + Vector3.new(0, 1, 0) + miss - from)
			shoot(c, weapon, tier, aim.Unit)
		end
		return
	end
	local goal = wander[c.model]
	if not goal or (c.root.Position - goal).Magnitude < 4 or math.random() < 0.02 then
		goal = randomInZone()
		for _, chest in loot:GetChildren() do
			if chest:IsA("BasePart") and chest:GetAttribute("Open") ~= true then
				local inRing = ((chest.Position - zoneCentre) * Vector3.new(1, 0, 1)).Magnitude < zoneRadius - 4
				if inRing and (chest.Position - c.root.Position).Magnitude < 50 then
					goal = chest.Position
					break
				end
			end
		end
		wander[c.model] = goal
	end
	Arena.go(c, goal)
end

------------------------------------------------------------------------

local function clearKit(c)
	kits[c.model] = nil
	show(c, "Shield", nil)
	show(c, "Wood", nil)
	show(c, "KOs", nil)
	local wing = c.model:FindFirstChild("Glider")
	if wing then
		wing:Destroy()
	end
	c.root.Anchored = false
end

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
	drawNext(nil, 0)
	resetIsland()
	table.clear(fighters)
	table.clear(kits)
	table.clear(lastHit)
	table.clear(wander)
	table.clear(lastWall)
	total = #people
	for _, c in people do
		Arena.reset(c)
		c.humanoid.Health = c.humanoid.MaxHealth
		fighters[c.model] = c
		setShield(c, 0)
		setWood(c, 0)
		show(c, "KOs", 0)
		if c.player then
			takeWeapons(c.player)
			give(c.player, "Sword")
		end
	end
	roundOn = true
	launch(people)

	local stage, stageStart, shrinking = 1, ship.start + SHIP_TIME, false
	local fromCentre, fromRadius, toCentre = zoneCentre, zoneRadius, zoneCentre
	local lastHurt = time()
	local nextThink = 0
	local function pickNext(s)
		local room = math.max(zoneRadius - s.radius, 0)
		local angle = math.random() * math.pi * 2
		local r = math.random() * room * 0.6
		return zoneCentre + Vector3.new(math.cos(angle) * r, 0, math.sin(angle) * r)
	end
	toCentre = pickNext(STAGES[1])
	drawNext(toCentre, STAGES[1].radius)
	while true do
		task.wait(0.1)
		local now = time()
		local list = standing()
		if #list <= 1 then
			break
		end

		local s = STAGES[math.min(stage, #STAGES)]
		if ship.on then
			Arena.status(#list .. " left")
			Arena.clock(ship.start + SHIP_TIME - now)
		elseif not shrinking then
			Arena.status("Phase " .. stage .. "  |  " .. #list .. " left")
			Arena.clock(stageStart + s.wait - now)
			if now >= stageStart + s.wait then
				shrinking, stageStart = true, now
				fromCentre, fromRadius = zoneCentre, zoneRadius
				Arena.tell("Storm closing")
			end
		else
			local t = math.clamp((now - stageStart) / s.shrink, 0, 1)
			Arena.status("Phase " .. stage .. "  |  " .. #list .. " left")
			Arena.clock(stageStart + s.shrink - now)
			zoneCentre = fromCentre:Lerp(toCentre, t)
			zoneRadius = fromRadius + (s.radius - fromRadius) * t
			drawZone()
			if t >= 1 then
				shrinking, stageStart = false, now
				stage += 1
				local after = STAGES[stage]
				if after then
					toCentre = pickNext(after)
					drawNext(toCentre, after.radius)
				else
					drawNext(nil, 0)
				end
			end
		end
		if now - lastHurt >= 1 then
			lastHurt = now
			for _, c in list do
				if landed(c) and outside(c) then
					hurt(c, s.hurt, nil)
				end
			end
		end

		openChests()
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
	parkShip()
	drawNext(nil, 0)
	Arena.clock(nil)
	local winner = standing()[1]
	if winner then
		Arena.bump(winner, "Wins")
		Arena.role(winner, "Winner")
		Arena.status(winner.name .. " wins")
		Arena.tell(winner.name .. " wins")
		if winner.player then
			resultEvent:FireClient(winner.player, 1, total, nil)
		end
	else
		Arena.status("No winner")
	end
	task.wait(6)
	for _, c in Arena.everyone() do
		clearKit(c)
		Arena.reset(c)
		if c.player then
			takeWeapons(c.player)
		end
		Arena.place(c, lobbySpot + Vector3.new(math.random(-8, 8), 0, math.random(-8, 8)))
	end
	builds:ClearAllChildren()
end

drawZone()
parkShip()
while true do
	Arena.status("Next round")
	Arena.countdown(BREAK)
	Arena.clock(nil)
	playRound()
end
