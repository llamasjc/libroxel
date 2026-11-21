-- The race: builds the chosen track, puts everyone in a kart on the grid
-- with computer racers to make eight, drives every kart, hands out items
-- from the boxes, keeps laps and places, and shows the results.
--
-- Screens talk to it through ReplicatedStorage.Kart: `Control` carries a
-- player's menu choices, drift button and item button to the server, and
-- `Race` tells screens about countdowns, laps, hits and results. What
-- every screen needs all the time (the phase, the clock, each kart's place,
-- lap and item) is kept in attributes on that folder and on the karts.

local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local storage = game:GetService("ReplicatedStorage")
local Lighting = game:GetService("Lighting")

local folder = storage:WaitForChild("Kart")
local Tracks = require(folder:WaitForChild("Tracks"))
local Control = folder:WaitForChild("Control")
local Race = folder:WaitForChild("Race")

local RACERS = 8
local TOP_SPEED = 62
local BOOST_SPEED = 92
local REVERSE_SPEED = 22
local GRAVITY = 85
local TURN_RATE = 2.3
local KART_RADIUS = 2.6
local POINTS = { 15, 12, 10, 8, 6, 4, 2, 1 }
local FINISH_WAIT = 30
local LEVELS = {
	Easy = { speed = 0.84, items = 1.6 },
	Normal = { speed = 0.93, items = 1.0 },
	Hard = { speed = 1.0, items = 0.6 },
}

local CPU_NAMES = { "Pip", "Rusty", "Juniper", "Bolt", "Marlow", "Tess", "Kiwi", "Dex", "Sunny", "Moss" }
local COLOURS = {
	Color3.fromRGB(230, 60, 60), Color3.fromRGB(60, 120, 230), Color3.fromRGB(250, 190, 40), Color3.fromRGB(60, 180, 90),
	Color3.fromRGB(160, 80, 220), Color3.fromRGB(250, 130, 40), Color3.fromRGB(60, 200, 210), Color3.fromRGB(240, 110, 180),
}

-- The items, and how likely each is by place (front, middle, back).
local ITEMS = {
	Turbo = { 20, 25, 15 },
	TripleTurbo = { 0, 12, 28 },
	Bumper = { 35, 25, 8 },
	Seeker = { 0, 18, 20 },
	Slime = { 45, 15, 4 },
	Overdrive = { 0, 3, 18 },
	Storm = { 0, 2, 9 },
}
local ITEM_ORDER = { "Turbo", "TripleTurbo", "Bumper", "Seeker", "Slime", "Overdrive", "Storm" }

folder:SetAttribute("Phase", "menu")
folder:SetAttribute("Track", 1)
folder:SetAttribute("Laps", 3)
folder:SetAttribute("Level", "Normal")
folder:SetAttribute("Clock", 0)
folder:SetAttribute("StartsIn", -1)
folder:SetAttribute("Countdown", -1)
folder:SetAttribute("FinishIn", -1)

local raceFolder = Instance.new("Folder")
raceFolder.Name = "Race"
raceFolder.Parent = workspace
local kartFolder = Instance.new("Folder")
kartFolder.Name = "Karts"
kartFolder.Parent = raceFolder
local thingsFolder = Instance.new("Folder")
thingsFolder.Name = "Thrown"
thingsFolder.Parent = raceFolder

local trackIndex = 0
local track = nil -- the built track's folder
local samples = nil
local groundParams = RaycastParams.new()
groundParams.FilterType = Enum.RaycastFilterType.Include

local racers = {}
local thrown = {}
local boxes = {}
local pads = {}
local votes = {} -- by UserId: track index
local raceClock = 0
local finishTimer = nil -- seconds left after the first racer finishes
local countdown = -1
local startTimer = -1 -- seconds until a race starts from the menu
local laps = 3
local level = LEVELS.Normal

------------------------------------------------------------------------
-- Building karts.

local function part(parent, props)
	local p = Instance.new("Part")
	p.Anchored = true
	p.CanCollide = false
	p.CanQuery = false
	p.CanTouch = false
	for key, value in props do
		p[key] = value
	end
	p.Parent = parent
	return p
end

-- A kart model facing -Z with its bottom at the origin. Players' karts get a
-- vehicle seat to sit in; computer racers get a driver.
local function makeKart(name, colour, withSeat)
	local model = Instance.new("Model")
	model.Name = name
	local body = part(model, { Name = "Body", Size = Vector3.new(4.6, 1.1, 6.6), Position = Vector3.new(0, 1.15, 0), Color = colour })
	model.PrimaryPart = body
	part(model, { Name = "Nose", Size = Vector3.new(3.6, 0.8, 2.2), Position = Vector3.new(0, 1.0, -4.1), Color = colour })
	part(model, { Name = "Bumper", Size = Vector3.new(5, 0.7, 0.6), Position = Vector3.new(0, 0.8, -5.3), Color = Color3.fromRGB(35, 35, 40) })
	part(model, { Name = "Engine", Size = Vector3.new(3, 1.4, 1.6), Position = Vector3.new(0, 2.1, 2.6), Color = Color3.fromRGB(70, 70, 78), Material = Enum.Material.Metal })
	part(model, { Name = "Spoiler", Size = Vector3.new(5.2, 0.3, 1.3), Position = Vector3.new(0, 3.4, 3.3), Color = colour })
	for side = -1, 1, 2 do
		part(model, { Name = "SpoilerPost", Size = Vector3.new(0.3, 1.2, 0.4), Position = Vector3.new(side * 1.6, 2.8, 3.3), Color = Color3.fromRGB(35, 35, 40) })
		part(model, { Name = "Pipe", Shape = Enum.PartType.Cylinder, Size = Vector3.new(1.2, 0.6, 0.6), CFrame = CFrame.new(side * 0.8, 1.6, 3.6) * CFrame.Angles(0, math.pi / 2, 0), Color = Color3.fromRGB(150, 150, 160), Material = Enum.Material.Metal })
	end
	for _, wheel in { { -2.6, -2.6, 1.9, 1.0 }, { 2.6, -2.6, 1.9, 1.0 }, { -2.7, 2.3, 2.3, 1.4 }, { 2.7, 2.3, 2.3, 1.4 } } do
		part(model, {
			Name = "Wheel",
			Shape = Enum.PartType.Cylinder,
			Size = Vector3.new(wheel[4], wheel[3], wheel[3]),
			Position = Vector3.new(wheel[1], wheel[3] / 2, wheel[2]),
			Color = Color3.fromRGB(28, 28, 30),
		})
	end
	part(model, { Name = "SteeringWheel", Shape = Enum.PartType.Cylinder, Size = Vector3.new(0.3, 1.4, 1.4), CFrame = CFrame.new(0, 2.4, -1.7) * CFrame.Angles(0, math.pi / 2, math.rad(-30)), Color = Color3.fromRGB(30, 30, 30) })
	if withSeat then
		local seat = Instance.new("VehicleSeat")
		seat.Name = "Seat"
		seat.Anchored = true
		seat.CanCollide = false
		seat.Size = Vector3.new(2.4, 0.6, 2.2)
		seat.Position = Vector3.new(0, 1.9, 0.6)
		seat.Color = Color3.fromRGB(40, 40, 45)
		seat.Parent = model
	else
		part(model, { Name = "Seat", Size = Vector3.new(2.4, 0.6, 2.2), Position = Vector3.new(0, 1.9, 0.6), Color = Color3.fromRGB(40, 40, 45) })
		part(model, { Name = "Driver", Size = Vector3.new(2.2, 2.2, 1.2), Position = Vector3.new(0, 3.3, 0.8), Color = colour:Lerp(Color3.new(1, 1, 1), 0.35) })
		part(model, { Name = "Helmet", Shape = Enum.PartType.Ball, Size = Vector3.new(2, 2, 2), Position = Vector3.new(0, 5.2, 0.8), Color = colour })
		part(model, { Name = "Visor", Size = Vector3.new(1.4, 0.6, 0.2), Position = Vector3.new(0, 5.3, -0.2), Color = Color3.fromRGB(30, 40, 60), Material = Enum.Material.Glass })
	end
	-- Sparks under the back wheels while drifting, and flames from the
	-- pipes while boosting.
	for _, side in { -1, 1 } do
		local at = Instance.new("Attachment")
		at.Name = "Sparks"
		at.Position = Vector3.new(side * 2.7, -1.0, 2.8)
		at.Parent = body
		local sparks = Instance.new("ParticleEmitter")
		sparks.Name = "Sparks"
		sparks.Enabled = false
		sparks.Rate = 60
		sparks.Lifetime = NumberRange.new(0.15, 0.3)
		sparks.Speed = NumberRange.new(6, 10)
		sparks.SpreadAngle = Vector2.new(50, 50)
		sparks.Size = NumberSequence.new(0.5)
		sparks.LightEmission = 1
		sparks.Parent = at
	end
	local exhaust = Instance.new("Attachment")
	exhaust.Name = "Exhaust"
	exhaust.Position = Vector3.new(0, 0.45, 4.3)
	exhaust.Parent = body
	local flame = Instance.new("ParticleEmitter")
	flame.Name = "Flame"
	flame.Enabled = false
	flame.Rate = 80
	flame.Lifetime = NumberRange.new(0.15, 0.3)
	flame.Speed = NumberRange.new(10, 16)
	flame.SpreadAngle = Vector2.new(12, 12)
	flame.Size = NumberSequence.new(1.1)
	flame.Color = ColorSequence.new(Color3.fromRGB(255, 170, 60))
	flame.LightEmission = 1
	flame.Parent = exhaust
	local tag = Instance.new("BillboardGui")
	tag.Name = "NameTag"
	tag.Size = UDim2.new(0, 160, 0, 30)
	tag.StudsOffset = Vector3.new(0, 7.5, 0)
	tag.MaxDistance = 150
	tag.Parent = body
	local text = Instance.new("TextLabel")
	text.Name = "Label"
	text.Size = UDim2.new(1, 0, 1, 0)
	text.BackgroundTransparency = 1
	text.Text = name
	text.TextColor3 = Color3.new(1, 1, 1)
	text.TextSize = 18
	text.Parent = tag
	local stroke = Instance.new("UIStroke")
	stroke.Thickness = 2
	stroke.Color = Color3.new(0, 0, 0)
	stroke.Parent = text
	return model
end

------------------------------------------------------------------------
-- Helpers.

local function forwardOf(heading)
	return Vector3.new(-math.sin(heading), 0, -math.cos(heading))
end

local function headingOf(v)
	return math.atan2(-v.X, -v.Z)
end

local function wrapAngle(a)
	return (a + math.pi) % (math.pi * 2) - math.pi
end

local function ordinal(n)
	local last = n % 10
	if n % 100 >= 11 and n % 100 <= 13 then
		return n .. "th"
	end
	return n .. (if last == 1 then "st" elseif last == 2 then "nd" elseif last == 3 then "rd" else "th")
end

local function tell(racer, ...)
	if racer.player and racer.player.Parent then
		Race:FireClient(racer.player, ...)
	end
end

local function humans()
	local list = {}
	for _, r in racers do
		if r.player then
			table.insert(list, r)
		end
	end
	return list
end

------------------------------------------------------------------------
-- Tracks.

local function buildTrack(index)
	if trackIndex == index and track then
		return
	end
	for _, r in racers do
		r.model:Destroy()
	end
	racers = {}
	if track then
		track:Destroy()
	end
	trackIndex = index
	track = Tracks.build(index, raceFolder)
	samples = Tracks.samples(index)
	-- Karts drive on the road, ramps and ground, never on walls or trees.
	groundParams.FilterDescendantsInstances = { track.Road, track.Ramps, track.Ground }
	Lighting.ClockTime = Tracks.list[index].sky
	boxes = {}
	for _, box in track.Boxes:GetChildren() do
		table.insert(boxes, { part = box, at = box.Position, away = 0 })
	end
	pads = {}
	for _, pad in track.Pads:GetChildren() do
		table.insert(pads, { cframe = pad.CFrame, size = pad.Size })
	end
	folder:SetAttribute("Track", index)
end

------------------------------------------------------------------------
-- Karts on the grid.

local function gridSpot(slot)
	local row = math.floor((slot - 1) / 2)
	local side = if slot % 2 == 1 then -1 else 1
	local back = 10 + row * 9 + (if side == 1 then 4.5 else 0)
	local p, t, n = Tracks.pointAt(samples, samples.length - back)
	return p + n * side * 6.5, headingOf(t)
end

local function newRacer(slot, name, colour, player)
	local model = makeKart(name, colour, player ~= nil)
	local at, heading = gridSpot(slot)
	local index, f = Tracks.locateAnywhere(samples, at)
	local r = {
		slot = slot,
		name = name,
		colour = colour,
		player = player,
		model = model,
		seat = model:FindFirstChild("Seat"),
		pos = at,
		heading = heading,
		look = heading,
		speed = 0,
		vy = 0,
		grounded = true,
		up = Vector3.yAxis,
		drift = 0,
		driftTime = 0,
		charge = 0,
		hopped = false,
		boost = 0,
		spin = 0,
		spinAngle = 0,
		overdrive = 0,
		slowed = 0,
		item = nil,
		uses = 0,
		rolling = 0,
		index = index,
		lapBase = if index > #samples / 2 then -#samples else 0,
		prog = 0,
		finished = false,
		time = 0,
		bestLap = 0,
		lapStart = 0,
		shownLap = 1,
		place = slot,
		wrongWay = 0,
		input = { throttle = 0, steer = 0, drift = false, use = false },
		heldSince = nil,
		air = 0,
		trick = false,
		ai = {
			lane = (slot % 3 - 1) * 5,
			laneGoal = 0,
			itemWait = 2,
			driftWait = 0,
			skill = 1,
		},
		attrs = {},
	}
	r.prog = r.lapBase + index - 1 + f
	model:SetAttribute("Racer", name)
	model:SetAttribute("UserId", if player then player.UserId else 0)
	model:SetAttribute("Colour", colour)
	model.Parent = kartFolder
	return r
end

local function seatPlayer(r)
	local character = r.player and r.player.Character
	local humanoid = character and character:FindFirstChildOfClass("Humanoid")
	if humanoid and r.seat and r.seat.Occupant ~= humanoid then
		-- No hopping out mid-race: Space drifts instead.
		humanoid.JumpPower = 0
		r.seat:Sit(humanoid)
	end
end

-- Puts the kart's model where the kart is: wheels on the ground, tilted
-- with the road and turned the way the body faces.
local function place(r)
	local up = r.up
	local look = forwardOf(r.look)
	look = (look - up * look:Dot(up)).Unit
	local right = look:Cross(up)
	local cf = CFrame.fromMatrix(r.pos, right, up)
	if r.spinAngle ~= 0 then
		cf *= CFrame.Angles(0, r.spinAngle, 0)
	end
	r.model:PivotTo(cf * CFrame.new(0, 1.15, 0))
end

local function setAttr(r, name, value)
	if r.attrs[name] ~= value then
		r.attrs[name] = value
		r.model:SetAttribute(name, value)
	end
end

local function setupRace()
	buildTrack(folder:GetAttribute("Track"))
	for _, r in racers do
		r.model:Destroy()
	end
	racers = {}
	for _, t in thrown do
		t.part:Destroy()
	end
	thrown = {}
	for _, box in boxes do
		box.away = 0
		box.part.Transparency = 0.35
	end
	laps = folder:GetAttribute("Laps")
	level = LEVELS[folder:GetAttribute("Level")] or LEVELS.Normal
	local slot = 0
	local people = Players:GetPlayers()
	-- People start at the back, as in the arcade.
	local first = math.max(1, RACERS - #people + 1)
	for i, player in people do
		if i > RACERS then
			break
		end
		local r = newRacer(first + i - 1, player.Name, COLOURS[(i - 1) % #COLOURS + 1], player)
		table.insert(racers, r)
	end
	local names = table.clone(CPU_NAMES)
	for k = 1, RACERS - #racers do
		slot = k
		local name = table.remove(names, (k * 7 + trackIndex) % #names + 1)
		local r = newRacer(slot, name, COLOURS[(#people + k - 1) % #COLOURS + 1], nil)
		r.ai.skill = level.speed * (1 - (k - 1) * 0.012)
		table.insert(racers, r)
	end
	for _, r in racers do
		place(r)
		seatPlayer(r)
	end
	raceClock = 0
	finishTimer = nil
	folder:SetAttribute("Clock", 0)
	folder:SetAttribute("FinishIn", -1)
end

------------------------------------------------------------------------
-- Items.

local function pickItem(r)
	local band = (r.place - 1) / math.max(#racers - 1, 1) * 2 -- 0 front, 1 middle, 2 back
	local low = math.floor(band)
	local f = band - low
	local total = 0
	local weights = {}
	for _, name in ITEM_ORDER do
		local w = ITEMS[name]
		local weight = w[low + 1] * (1 - f) + (w[math.min(low + 2, 3)] or 0) * f
		weights[name] = weight
		total += weight
	end
	local roll = math.random() * total
	for _, name in ITEM_ORDER do
		roll -= weights[name]
		if roll <= 0 then
			return name
		end
	end
	return "Turbo"
end

local function hit(r, by, how)
	if r.overdrive > 0 or r.finished and not r.player then
		return false
	end
	if r.spin > 0 then
		return false
	end
	r.spin = 1.1
	r.drift = 0
	r.charge = 0
	r.boost = 0
	r.speed *= 0.35
	tell(r, "hit", how, by and by.name or nil)
	if by and by ~= r then
		tell(by, "landed", how, r.name)
	end
	return true
end

local function throwPart(props)
	local p = part(thingsFolder, props)
	return p
end

local function useItem(r)
	local item = r.item
	if not item or r.rolling > 0 or r.spin > 0 then
		return
	end
	r.uses -= 1
	if r.uses <= 0 then
		r.item = nil
	end
	local fwd = forwardOf(r.heading)
	if item == "Turbo" or item == "TripleTurbo" then
		r.boost = math.max(r.boost, 1.3)
		tell(r, "boost")
	elseif item == "Overdrive" then
		r.overdrive = 7
		r.boost = 0
		tell(r, "boost")
	elseif item == "Slime" then
		local at = r.pos - fwd * 6
		table.insert(thrown, {
			kind = "Slime",
			pos = at,
			life = 45,
			owner = r,
			safe = 0.5,
			part = throwPart({ Name = "Slime", Shape = Enum.PartType.Cylinder, Size = Vector3.new(0.5, 5, 5), CFrame = CFrame.new(at + Vector3.new(0, 0.3, 0)) * CFrame.Angles(0, 0, math.pi / 2), Color = Color3.fromRGB(120, 220, 60), Material = Enum.Material.Neon }),
		})
	elseif item == "Bumper" then
		local back = r.input.throttle < 0
		local heading = if back then r.heading + math.pi else r.heading
		local at = r.pos + forwardOf(heading) * 6 + Vector3.new(0, 1.2, 0)
		table.insert(thrown, {
			kind = "Bumper",
			pos = at,
			heading = heading,
			speed = math.max(r.speed, 0) + 45,
			index = r.index,
			life = 7,
			owner = r,
			safe = 0.4,
			part = throwPart({ Name = "Bumper", Shape = Enum.PartType.Ball, Size = Vector3.new(2.4, 2.4, 2.4), Position = at, Color = Color3.fromRGB(60, 210, 120), Material = Enum.Material.Neon }),
		})
	elseif item == "Seeker" then
		-- Chases whoever is one place ahead, round the road.
		local target = nil
		for _, other in racers do
			if other.place == r.place - 1 then
				target = other
			end
		end
		local at = r.pos + Vector3.new(0, 2, 0)
		table.insert(thrown, {
			kind = "Seeker",
			pos = at,
			distance = samples[r.index].at + (r.prog % 1) * samples[r.index].len + 4,
			side = 0,
			target = target,
			life = 12,
			owner = r,
			safe = 0.6,
			part = throwPart({ Name = "Seeker", Shape = Enum.PartType.Ball, Size = Vector3.new(2.6, 2.6, 2.6), Position = at, Color = Color3.fromRGB(240, 60, 80), Material = Enum.Material.Neon }),
		})
	elseif item == "Storm" then
		for _, other in racers do
			if other.place < r.place and not other.finished then
				if hit(other, r, "Storm") then
					other.slowed = 3
				end
			end
		end
		Race:FireAllClients("storm", r.name)
	end
end

------------------------------------------------------------------------
-- Driving.

local function readInput(r, dt)
	if r.player and not r.finished then
		if r.player.Parent ~= Players then
			r.player = nil
		else
			r.input.throttle = r.seat and r.seat.Throttle or 0
			r.input.steer = r.seat and r.seat.Steer or 0
			return
		end
	end
	-- The computer drives: aim at a point a little way down the road, in
	-- its own lane, and drift through the bigger bends.
	local ai = r.ai
	local here = samples[r.index].at + (r.prog % 1) * samples[r.index].len
	ai.laneGoal = if math.random() < dt * 0.3 then (math.random() * 2 - 1) * (Tracks.ROAD_HALF - 5) else ai.laneGoal
	ai.lane += math.clamp(ai.laneGoal - ai.lane, -4 * dt, 4 * dt)
	local p, _, n = Tracks.pointAt(samples, here + 16 + math.max(r.speed, 0) * 0.3)
	local target = p + n * ai.lane
	local want = headingOf(target - r.pos)
	local diff = wrapAngle(want - r.heading)
	local far = Tracks.pointAt(samples, here + 70)
	local bend = wrapAngle(headingOf(far - r.pos) - r.heading)
	r.input.steer = math.clamp(-diff * 2.6, -1, 1)
	r.input.throttle = if math.abs(diff) > 1.0 and r.speed > 35 then 0.4 else 1
	ai.driftWait -= dt
	if r.drift == 0 then
		r.input.drift = math.abs(bend) > 0.55 and r.speed > 40 and ai.driftWait <= 0 and ai.skill > 0.85
	elseif math.abs(bend) < 0.18 and r.driftTime > 0.7 or r.driftTime > 3.6 then
		r.input.drift = false
		ai.driftWait = 1
	end
	-- Items: after a moment, when it makes sense.
	if r.item and r.rolling <= 0 then
		ai.itemWait -= dt
		if ai.itemWait <= 0 then
			local use = true
			if r.item == "Bumper" then
				use = false
				for _, other in racers do
					local to = other.pos - r.pos
					if other ~= r and to.Magnitude < 70 and to.Unit:Dot(forwardOf(r.heading)) > 0.94 then
						use = true
					end
				end
				use = use or ai.itemWait < -6
			elseif r.item == "Turbo" or r.item == "TripleTurbo" or r.item == "Overdrive" then
				use = math.abs(bend) < 0.25
			end
			if use then
				useItem(r)
				ai.itemWait = 0.6 + math.random() * 2 * level.items
			end
		end
	end
end

local function physics(r, dt)
	local input = r.input
	local throttle, steer = input.throttle, input.steer
	if r.spin > 0 then
		throttle, steer = 0, 0
	end
	local skill = if r.player then 1 else r.ai.skill
	-- Computer racers keep up with people, a little.
	if not r.player then
		local best = nil
		for _, other in racers do
			if other.player and (not best or other.prog > best) then
				best = other.prog
			end
		end
		if best then
			local gap = (best - r.prog) / #samples
			skill *= 1 + math.clamp(gap * 0.25, -0.06, 0.07)
		end
	end
	local offRoad = false
	local _, _, side = Tracks.locate(samples, r.pos, r.index)
	if math.abs(side) > Tracks.ROAD_HALF + 1.5 and r.grounded then
		offRoad = true
	end
	local top = TOP_SPEED * skill
	if r.slowed > 0 then
		top *= 0.6
	end
	if offRoad and r.boost <= 0 and r.overdrive <= 0 then
		top *= 0.45
	end
	local boosting = r.boost > 0 or r.overdrive > 0
	if boosting then
		top = BOOST_SPEED * (if r.overdrive > 0 then 1.05 else 1)
	end

	-- Speed.
	if boosting then
		r.speed = math.min(top, r.speed + 160 * dt)
	elseif throttle > 0 then
		local goal = top * throttle
		if r.speed < goal then
			r.speed = math.min(goal, r.speed + (14 + 34 * (1 - r.speed / top)) * dt)
		else
			r.speed = math.max(goal, r.speed - 50 * dt)
		end
	elseif throttle < 0 then
		if r.speed > 0 then
			r.speed = math.max(-1, r.speed - 85 * dt)
		else
			r.speed = math.max(-REVERSE_SPEED, r.speed - 28 * dt)
		end
	else
		local drag = (if r.spin > 0 then 70 else 20) * dt
		r.speed -= math.clamp(r.speed, -drag, drag)
	end
	if r.speed > top and not boosting then
		r.speed = math.max(top, r.speed - 60 * dt)
	end

	-- Drifting: press drift while steering to hop into a slide. The longer
	-- you hold it, steering into the bend, the bigger the boost when you let go.
	if r.drift == 0 then
		if input.drift and not r.hopped and r.grounded and r.spin <= 0 and throttle ~= 0 then
			r.hopped = true
			r.vy = 10
			r.grounded = false
		end
		if input.drift and r.hopped and math.abs(steer) > 0.2 and r.speed > 22 then
			r.drift = if steer > 0 then 1 else -1
			r.driftTime = 0
			r.charge = 0
		end
		if not input.drift then
			r.hopped = false
		end
	else
		if not input.drift or r.speed < 14 or r.spin > 0 then
			local tier = if r.charge >= 2.9 then 3 elseif r.charge >= 1.9 then 2 elseif r.charge >= 0.9 then 1 else 0
			if tier > 0 then
				r.boost = math.max(r.boost, ({ 0.6, 1.1, 1.7 })[tier])
				tell(r, "miniturbo", tier)
			end
			r.drift = 0
			r.charge = 0
			r.hopped = false
		else
			r.driftTime += dt
			local into = steer * r.drift
			r.charge += dt * (if into > 0.3 then 1.35 else 0.75)
		end
	end

	-- Turning.
	if r.spin > 0 then
		r.spinAngle += dt * math.pi * 2 / 1.1
	elseif r.drift ~= 0 then
		local into = steer * r.drift
		r.heading -= r.drift * (1.25 + 0.75 * into) * dt
	else
		local grip = math.clamp(math.abs(r.speed) / 12, 0, 1) * (1 - 0.25 * math.clamp(r.speed / TOP_SPEED, 0, 1))
		r.heading -= steer * TURN_RATE * grip * dt * (if r.speed < 0 then -1 else 1)
	end
	-- The body leans into a drift.
	local lookGoal = r.heading - r.drift * 0.38
	r.look += wrapAngle(lookGoal - r.look) * math.min(1, 12 * dt)
	if r.spin <= 0 then
		r.spinAngle = 0
	end

	-- Moving, along the ground or through the air.
	local move = forwardOf(r.heading) * r.speed * dt
	local nextPos = r.pos + move
	local ground = workspace:Raycast(nextPos + Vector3.new(0, 3.5, 0), Vector3.new(0, -60, 0), groundParams)
	local groundY = if ground then ground.Position.Y else -100
	if r.grounded and groundY >= r.pos.Y - 0.7 then
		local rise = (groundY - r.pos.Y) / dt
		r.vy = math.clamp(rise, -40, 45)
		nextPos = Vector3.new(nextPos.X, groundY, nextPos.Z)
		r.up = r.up:Lerp(ground.Normal, math.min(1, 12 * dt)).Unit
		r.air = 0
	else
		if r.grounded then
			-- Off the end of a ramp, or over a crest.
			r.grounded = false
			r.trick = false
		end
		r.air += dt
		r.vy -= GRAVITY * dt
		nextPos = Vector3.new(nextPos.X, r.pos.Y + r.vy * dt, nextPos.Z)
		r.up = r.up:Lerp(Vector3.yAxis, math.min(1, 3 * dt)).Unit
		-- Press drift in a real jump for a trick, and a boost on landing.
		if input.drift and r.air > 0.25 and not r.trick and r.vy > -20 and not r.hopped then
			r.trick = true
			tell(r, "trick")
		end
		if nextPos.Y <= groundY then
			nextPos = Vector3.new(nextPos.X, groundY, nextPos.Z)
			r.grounded = true
			if r.trick then
				r.boost = math.max(r.boost, 0.8)
				r.trick = false
			end
			r.vy = 0
		end
	end
	r.pos = nextPos

	-- Walls keep karts on the track: slide along them, losing speed.
	local index, f, sideNow = Tracks.locate(samples, r.pos, r.index)
	local limit = Tracks.WALL - KART_RADIUS
	if math.abs(sideNow) > limit then
		local s = samples[index]
		local n = s.n:Lerp(samples[index % #samples + 1].n, f).Unit
		local sign = if sideNow > 0 then 1 else -1
		r.pos -= n * (math.abs(sideNow) - limit) * sign
		local v = forwardOf(r.heading) * r.speed
		local into = v:Dot(n) * sign
		if into > 0 then
			local along = v - n * v:Dot(n)
			if along.Magnitude > 1 then
				r.heading = headingOf(along * (if r.speed < 0 then -1 else 1))
			end
			r.speed *= math.clamp(1 - into / 60, 0.4, 1)
			if into > 20 then
				tell(r, "bonk")
			end
		end
	end

	-- Where it is round the lap.
	local count = #samples
	if r.index > count - 6 and index < 6 then
		r.lapBase += count
	elseif r.index < 6 and index > count - 6 then
		r.lapBase -= count
	end
	r.index = index
	r.prog = r.lapBase + index - 1 + f

	-- Wrong way?
	local t = samples[index].t
	if forwardOf(r.heading):Dot(t) < -0.4 and r.speed > 8 then
		r.wrongWay += dt
	else
		r.wrongWay = 0
	end

	-- Fell off somehow: back onto the road.
	if r.pos.Y < -30 then
		local p, tt = Tracks.pointAt(samples, samples[index].at)
		r.pos = p + Vector3.new(0, 4, 0)
		r.heading = headingOf(tt)
		r.speed = 0
		r.vy = 0
	end

	r.boost = math.max(0, r.boost - dt)
	r.spin = math.max(0, r.spin - dt)
	r.overdrive = math.max(0, r.overdrive - dt)
	r.slowed = math.max(0, r.slowed - dt)
end

-- Karts bump each other apart; one in overdrive knocks the other spinning.
local function bumps()
	for i = 1, #racers do
		local a = racers[i]
		for j = i + 1, #racers do
			local b = racers[j]
			local d = b.pos - a.pos
			local flat = Vector3.new(d.X, 0, d.Z)
			local dist = flat.Magnitude
			if dist < KART_RADIUS * 2 and dist > 0.01 and math.abs(d.Y) < 4 then
				local push = flat.Unit * (KART_RADIUS * 2 - dist) / 2
				a.pos -= push
				b.pos += push
				if a.overdrive > 0 and b.overdrive <= 0 then
					hit(b, a, "Overdrive")
				elseif b.overdrive > 0 and a.overdrive <= 0 then
					hit(a, b, "Overdrive")
				else
					local slow = math.min(a.speed, b.speed)
					a.speed = (a.speed * 3 + slow) / 4
					b.speed = (b.speed * 3 + slow) / 4
				end
			end
		end
	end
end

local function pickups(dt)
	for _, box in boxes do
		if box.away > 0 then
			box.away -= dt
			if box.away <= 0 then
				box.part.Transparency = 0.35
				box.part.Decal.Transparency = 0
			end
		end
	end
	for _, r in racers do
		for _, box in boxes do
			if box.away <= 0 and (box.at - r.pos - Vector3.new(0, 2, 0)).Magnitude < 5.5 then
				box.away = 3
				box.part.Transparency = 1
				box.part.Decal.Transparency = 1
				if not r.item and r.rolling <= 0 then
					r.rolling = 1.4
					tell(r, "roll")
				end
			end
		end
		if r.rolling > 0 then
			r.rolling -= dt
			if r.rolling <= 0 then
				r.item = pickItem(r)
				r.uses = if r.item == "TripleTurbo" then 3 else 1
				tell(r, "item", r.item)
			end
		end
		for _, pad in pads do
			local localPos = pad.cframe:PointToObjectSpace(r.pos)
			if math.abs(localPos.X) < pad.size.X / 2 + 1 and math.abs(localPos.Z) < pad.size.Z / 2 + 1 and math.abs(localPos.Y) < 3 then
				if r.boost < 0.5 then
					tell(r, "boost")
				end
				r.boost = math.max(r.boost, 1)
			end
		end
		if r.player and r.input.use then
			r.input.use = false
			useItem(r)
		end
	end
end

local function moveThrown(dt)
	for k = #thrown, 1, -1 do
		local t = thrown[k]
		t.life -= dt
		t.safe -= dt
		if t.kind == "Bumper" then
			local nextPos = t.pos + forwardOf(t.heading) * t.speed * dt
			local index, f, side = Tracks.locate(samples, nextPos, t.index)
			t.index = index
			local s = samples[index]
			local n = s.n
			if math.abs(side) > Tracks.WALL - 1.5 then
				-- Bounces off walls.
				local v = forwardOf(t.heading)
				v -= n * 2 * v:Dot(n)
				t.heading = headingOf(v)
				nextPos -= n * (math.abs(side) - (Tracks.WALL - 1.5)) * math.sign(side)
			end
			local ground = workspace:Raycast(nextPos + Vector3.new(0, 4, 0), Vector3.new(0, -40, 0), groundParams)
			t.pos = Vector3.new(nextPos.X, (if ground then ground.Position.Y else nextPos.Y - 1.2) + 1.2, nextPos.Z)
		elseif t.kind == "Seeker" then
			local target = t.target
			t.distance += 105 * dt
			if target and target.model.Parent then
				local here = samples[target.index].at + (target.prog % 1) * samples[target.index].len
				local gap = (here - t.distance) % samples.length
				local _, _, targetSide = Tracks.locate(samples, target.pos, target.index)
				t.side += (targetSide - t.side) * math.min(1, dt * 2)
				if gap < 14 or gap > samples.length - 4 then
					t.pos = t.pos:Lerp(target.pos + Vector3.new(0, 1.5, 0), math.min(1, dt * 12))
				else
					local p, _, n = Tracks.pointAt(samples, t.distance)
					t.pos = p + n * t.side + Vector3.new(0, 2, 0)
				end
			else
				local p = Tracks.pointAt(samples, t.distance)
				t.pos = p + Vector3.new(0, 2, 0)
			end
		end
		local gone = t.life <= 0
		if not gone then
			for _, r in racers do
				if (r ~= t.owner or t.safe <= 0) and (r.pos + Vector3.new(0, 1.5, 0) - t.pos).Magnitude < (if t.kind == "Slime" then 4 else 3.8) then
					if t.kind ~= "Seeker" or r == t.target or (r.pos - t.pos).Magnitude < 3 then
						hit(r, t.owner, t.kind)
						gone = true
						break
					end
				end
			end
		end
		-- Two thrown things that meet knock each other out.
		if not gone and t.kind ~= "Seeker" then
			for j = #thrown, 1, -1 do
				local other = thrown[j]
				if other ~= t and other.kind ~= "Seeker" and (other.pos - t.pos).Magnitude < 3.5 and other.life > 0 then
					other.life = 0
					gone = true
					break
				end
			end
		end
		if gone then
			t.part:Destroy()
			table.remove(thrown, k)
		else
			if t.kind == "Slime" then
				t.part.CFrame = CFrame.new(t.pos + Vector3.new(0, 0.3, 0)) * CFrame.Angles(0, 0, math.pi / 2)
			else
				t.part.Position = t.pos
			end
		end
	end
end

local function standings()
	table.sort(racers, function(a, b)
		if a.finished ~= b.finished then
			return a.finished
		end
		if a.finished then
			return a.time < b.time
		end
		return a.prog > b.prog
	end)
	for i, r in racers do
		r.place = i
	end
end

local function lapsAndFinish(r)
	local count = #samples
	local lap = math.clamp(math.floor(r.prog / count) + 1, 1, laps)
	if not r.finished and r.prog >= laps * count then
		r.finished = true
		r.time = raceClock
		local lapTime = raceClock - r.lapStart
		if r.bestLap == 0 or lapTime < r.bestLap then
			r.bestLap = lapTime
		end
		tell(r, "finish", r.place, raceClock)
		if finishTimer == nil then
			finishTimer = FINISH_WAIT
		end
		-- A person who finishes is driven round by the computer from here.
		r.ai.lane = 0
		r.ai.skill = 0.8
	elseif lap > r.shownLap and not r.finished then
		local lapTime = raceClock - r.lapStart
		if r.bestLap == 0 or lapTime < r.bestLap then
			r.bestLap = lapTime
		end
		r.lapStart = raceClock
		r.shownLap = lap
		tell(r, "lap", lap, laps)
	end
	setAttr(r, "Lap", lap)
end

local function publish(r)
	setAttr(r, "Place", r.place)
	setAttr(r, "Item", r.item or "")
	setAttr(r, "Uses", r.uses)
	setAttr(r, "Rolling", r.rolling > 0)
	setAttr(r, "Charge", if r.drift == 0 then 0 elseif r.charge >= 2.9 then 3 elseif r.charge >= 1.9 then 2 elseif r.charge >= 0.9 then 1 else 0)
	setAttr(r, "Boost", r.boost > 0 or r.overdrive > 0)
	setAttr(r, "Overdrive", r.overdrive > 0)
	setAttr(r, "Finished", r.finished)
	setAttr(r, "Time", math.floor(r.time * 100) / 100)
	setAttr(r, "WrongWay", r.wrongWay > 1.2 and r.player ~= nil)
	setAttr(r, "Speed", math.floor(math.abs(r.speed) * 1.6))
	setAttr(r, "Heading", math.floor(r.heading * 1000) / 1000)
	setAttr(r, "Distance", math.floor((r.prog % #samples) * 10) / 10)
	local charge = r.attrs.Charge
	local colour = if charge == 3 then Color3.fromRGB(240, 90, 255) elseif charge == 2 then Color3.fromRGB(255, 160, 40) else Color3.fromRGB(80, 180, 255)
	local body = r.model.PrimaryPart
	for _, a in body:GetChildren() do
		if a.Name == "Sparks" then
			local sparks = a.Sparks
			local on = charge > 0 and r.grounded
			if sparks.Enabled ~= on then
				sparks.Enabled = on
			end
			if on and r.attrs.SparkColour ~= charge then
				sparks.Color = ColorSequence.new(colour)
			end
		end
	end
	r.attrs.SparkColour = charge
	local flame = body.Exhaust.Flame
	local boosting = r.boost > 0 or r.overdrive > 0
	if flame.Enabled ~= boosting then
		flame.Enabled = boosting
	end
	local glow = r.overdrive > 0
	local highlight = r.model:FindFirstChild("Glow")
	if glow and not highlight then
		highlight = Instance.new("Highlight")
		highlight.Name = "Glow"
		highlight.FillColor = Color3.fromRGB(255, 230, 90)
		highlight.FillTransparency = 0.4
		highlight.OutlineColor = Color3.fromRGB(255, 255, 255)
		highlight.Parent = r.model
	elseif not glow and highlight then
		highlight:Destroy()
	end
end

local function results()
	local list = {}
	for _, r in racers do
		local points = POINTS[r.place] or 0
		if r.player and r.player.Parent then
			local stats = r.player:FindFirstChild("leaderstats")
			if stats then
				stats.Points.Value += points
				if r.place == 1 then
					stats.Wins.Value += 1
				end
			end
		end
		table.insert(list, {
			place = r.place,
			name = r.name,
			person = r.player ~= nil,
			userId = if r.player then r.player.UserId else 0,
			time = if r.finished then r.time else -1,
			best = r.bestLap,
			points = points,
			colour = r.colour,
		})
	end
	return list
end

------------------------------------------------------------------------
-- The phases: menu, countdown, race, results.

local function setPhase(phase)
	folder:SetAttribute("Phase", phase)
end

local function startCountdown()
	setupRace()
	setPhase("countdown")
	countdown = 3.999
	Race:FireAllClients("countdown")
	for _, r in racers do
		r.heldSince = nil
		if not r.player and folder:GetAttribute("Level") == "Hard" and math.random() < 0.5 then
			r.heldSince = 1.2
		end
	end
end

local function finishRace()
	setPhase("results")
	standings()
	local list = results()
	folder:SetAttribute("ResultsAt", time())
	Race:FireAllClients("results", list)
	for _, r in racers do
		if r.player then
			local character = r.player.Character
			local humanoid = character and character:FindFirstChildOfClass("Humanoid")
			if humanoid then
				humanoid.JumpPower = 50
			end
		end
	end
end

-- The track with the most votes wins; ties go to the one already showing.
local function chosenTrack()
	local tally = {}
	for _, index in votes do
		tally[index] = (tally[index] or 0) + 1
	end
	local best, bestCount = folder:GetAttribute("Track"), tally[folder:GetAttribute("Track")] or 0
	for index, n in tally do
		if n > bestCount then
			best, bestCount = index, n
		end
	end
	return best
end

local function showVotes()
	local tally = {}
	for _, index in votes do
		tally[index] = (tally[index] or 0) + 1
	end
	for i = 1, #Tracks.list do
		folder:SetAttribute("Votes" .. i, tally[i] or 0)
	end
end

Control.OnServerEvent:Connect(function(player, what, value)
	local phase = folder:GetAttribute("Phase")
	if what == "drift" or what == "item" then
		for _, r in racers do
			if r.player == player then
				if what == "drift" then
					r.input.drift = value == true
				else
					r.input.use = true
				end
			end
		end
	elseif phase == "menu" or phase == "results" then
		if what == "track" and Tracks.list[value] then
			votes[player.UserId] = value
			showVotes()
			if phase == "menu" then
				buildTrack(chosenTrack())
			end
		elseif what == "level" and LEVELS[value] then
			folder:SetAttribute("Level", value)
		elseif what == "laps" and (value == 1 or value == 3 or value == 5) then
			folder:SetAttribute("Laps", value)
		elseif what == "menu" and phase == "results" then
			setPhase("menu")
			folder:SetAttribute("StartsIn", -1)
			startTimer = -1
		elseif what == "start" then
			if phase == "results" then
				setPhase("menu")
			end
			if folder:GetAttribute("StartsIn") < 0 then
				-- On your own it starts straight away; with others, they get
				-- a few seconds to vote.
				folder:SetAttribute("StartsIn", if #Players:GetPlayers() > 1 then 5 else 0)
			end
		end
	end
end)

Players.PlayerAdded:Connect(function(player)
	local stats = Instance.new("Folder")
	stats.Name = "leaderstats"
	stats.Parent = player
	local points = Instance.new("IntValue")
	points.Name = "Points"
	points.Parent = stats
	local wins = Instance.new("IntValue")
	wins.Name = "Wins"
	wins.Parent = stats
end)

Players.PlayerRemoving:Connect(function(player)
	votes[player.UserId] = nil
	showVotes()
	for _, r in racers do
		if r.player == player then
			r.player = nil
			r.model:SetAttribute("UserId", 0)
		end
	end
end)

local function update(dt)
	local phase = folder:GetAttribute("Phase")
	if phase == "menu" then
		local startsIn = folder:GetAttribute("StartsIn")
		if startsIn >= 0 then
			if startTimer < 0 then
				startTimer = startsIn
			end
			startTimer -= dt
			startsIn = startTimer
			if startsIn <= 0 then
				startTimer = -1
				folder:SetAttribute("StartsIn", -1)
				buildTrack(chosenTrack())
				votes = {}
				showVotes()
				startCountdown()
			else
				folder:SetAttribute("StartsIn", math.ceil(startsIn * 10) / 10)
			end
		end
		return
	end
	if phase == "results" then
		if time() - folder:GetAttribute("ResultsAt") > 25 then
			setPhase("menu")
		end
		return
	end
	if phase == "countdown" then
		local before = math.ceil(countdown)
		countdown -= dt
		for _, r in racers do
			readInput(r, dt)
			seatPlayer(r)
			-- Hold the throttle from about "2" for a rocket start.
			if r.input.throttle > 0 then
				r.heldSince = r.heldSince or countdown
			else
				r.heldSince = nil
			end
		end
		if math.ceil(countdown) ~= before and countdown > 0 then
			folder:SetAttribute("Countdown", math.ceil(countdown))
		end
		if countdown <= 0 then
			folder:SetAttribute("Countdown", 0)
			setPhase("race")
			for _, r in racers do
				if r.heldSince and r.heldSince < 2.2 then
					r.boost = 1
					tell(r, "rocket")
				end
				r.lapStart = 0
			end
			Race:FireAllClients("go")
		end
		for _, r in racers do
			publish(r)
		end
		return
	end
	-- Racing.
	raceClock += dt
	folder:SetAttribute("Clock", math.floor(raceClock * 100) / 100)
	for _, r in racers do
		readInput(r, dt)
		physics(r, dt)
	end
	bumps()
	pickups(dt)
	moveThrown(dt)
	standings()
	local everyoneDone = true
	local anyPerson = false
	for _, r in racers do
		lapsAndFinish(r)
		if r.player then
			anyPerson = true
			if not r.finished then
				everyoneDone = false
			end
		end
	end
	for _, r in racers do
		place(r)
		publish(r)
		if r.player and r.seat and r.seat.Occupant == nil then
			seatPlayer(r)
		end
	end
	if finishTimer then
		finishTimer -= dt
		folder:SetAttribute("FinishIn", math.max(0, math.ceil(finishTimer)))
	end
	-- Over when every person has finished, when the time after the first
	-- finisher runs out, or (with nobody racing) after ten minutes.
	local timeUp = finishTimer ~= nil and finishTimer <= 0
	if (anyPerson and everyoneDone) or timeUp or (not anyPerson and raceClock > 600) then
		finishRace()
	end
end

buildTrack(1)
showVotes()

RunService.Heartbeat:Connect(function(dt)
	-- Small steps, so fast karts don't skip through walls or boxes.
	dt = math.min(dt, 0.1)
	local steps = math.max(1, math.ceil(dt / (1 / 60) - 0.01))
	for _ = 1, steps do
		update(dt / steps)
	end
end)
