-- The race tracks, and how to build one.
--
-- Each track is a loop of control points ({ x, z, height }), smoothed into
-- samples every few studs. The road, kerbs, walls, ramps, boost pads and
-- item boxes are all built from those samples when a race is set up, so a
-- new track is only a new list of points. The race starts at the first
-- point, heading for the second.

local Tracks = {}

-- How wide the road is, and how far from its middle the walls stand
-- (the strip between is slow grass, sand or snow).
Tracks.ROAD_HALF = 16
Tracks.WALL = 25
Tracks.SPACING = 10

local function figureEight()
	local points = {}
	local count = 28
	for i = 0, count - 1 do
		local t = i / count * math.pi * 2
		-- Over the crossing on the second half: up and over a bridge.
		local away = math.abs(((t - math.pi + math.pi) % (math.pi * 2)) - math.pi)
		local height = 0
		if away < 0.55 then
			height = 16
		elseif away < 1.25 then
			height = 16 * (1 - (away - 0.55) / 0.7)
		end
		table.insert(points, { 330 * math.sin(t), 210 * math.sin(2 * t), height })
	end
	return points
end

Tracks.list = {
	{
		name = "Sunny Meadow",
		blurb = "Wide bends, a jump and a sneaky S-bend",
		ground = { Color3.fromRGB(104, 168, 80), Enum.Material.Grass },
		shoulder = Color3.fromRGB(122, 180, 92),
		road = Color3.fromRGB(70, 72, 80),
		sky = 14,
		decor = "trees",
		points = {
			{ 0, 200 }, { 150, 200 }, { 255, 175 }, { 305, 90 }, { 265, 0 }, { 175, -35 },
			{ 120, -110 }, { 165, -190 }, { 80, -265 }, { -60, -260 }, { -150, -180 },
			{ -110, -95 }, { -205, -30 }, { -290, 55 }, { -255, 165 }, { -140, 205 },
		},
		boxes = { 0.17, 0.47, 0.8 },
		pads = { { 0.33, -6 }, { 0.64, 6 }, { 0.93, 0 } },
		ramps = { { 0.56, 0 } },
	},
	{
		name = "Dusty Canyon",
		blurb = "A figure eight with a big bridge over the middle",
		ground = { Color3.fromRGB(214, 184, 128), Enum.Material.Sand },
		shoulder = Color3.fromRGB(196, 160, 104),
		road = Color3.fromRGB(110, 86, 70),
		sky = 16,
		decor = "cacti",
		points = figureEight(),
		boxes = { 0.12, 0.4, 0.62, 0.88 },
		pads = { { 0.25, 0 }, { 0.75, 0 } },
		ramps = {},
	},
	{
		name = "Frosty Peaks",
		blurb = "Tight hairpins, a hill and an icy jump",
		ground = { Color3.fromRGB(236, 242, 250), Enum.Material.Snow },
		shoulder = Color3.fromRGB(214, 226, 240),
		road = Color3.fromRGB(58, 64, 82),
		sky = 9,
		decor = "pines",
		points = {
			{ 0, -255 }, { 190, -255 }, { 295, -185 }, { 300, -65, 4 }, { 205, -10, 10 },
			{ 125, 50, 10 }, { 200, 120, 4 }, { 300, 175 }, { 265, 270 }, { 130, 275 },
			{ 30, 215 }, { -70, 265 }, { -205, 265 }, { -300, 170 }, { -265, 45 },
			{ -150, 15 }, { -120, -80 }, { -240, -140 }, { -195, -245 },
		},
		boxes = { 0.15, 0.42, 0.7 },
		pads = { { 0.06, 0 }, { 0.5, -6 }, { 0.86, 6 } },
		ramps = { { 0.66, 0 } },
	},
}

------------------------------------------------------------------------
-- Samples along the middle of the road.

local function catmull(p0, p1, p2, p3, t)
	local t2, t3 = t * t, t * t * t
	return 0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t2 + (-p0 + 3 * p1 - 3 * p2 + p3) * t3)
end

local cache = {}

-- The track's samples: each has `p` (the middle of the road, on its top),
-- `t` (the way the road goes, flat), `n` (to the right of it), `len` (to
-- the next sample) and `at` (how far round the lap it is).
function Tracks.samples(index)
	if cache[index] then
		return cache[index]
	end
	local track = Tracks.list[index]
	local control = {}
	for _, point in track.points do
		table.insert(control, Vector3.new(point[1], point[3] or 0, point[2]))
	end
	local count = #control
	-- A fine curve first, then even steps along it.
	local fine = {}
	for i = 1, count do
		local p0 = control[(i - 2) % count + 1]
		local p1 = control[i]
		local p2 = control[i % count + 1]
		local p3 = control[(i + 1) % count + 1]
		for s = 0, 23 do
			table.insert(fine, catmull(p0, p1, p2, p3, s / 24))
		end
	end
	local total = 0
	for i = 1, #fine do
		total += (fine[i % #fine + 1] - fine[i]).Magnitude
	end
	local steps = math.floor(total / Tracks.SPACING + 0.5)
	local step = total / steps
	local samples = {}
	local walked, want, i = 0, 0, 1
	while #samples < steps do
		local a, b = fine[i], fine[i % #fine + 1]
		local length = (b - a).Magnitude
		if walked + length >= want then
			local p = a:Lerp(b, (want - walked) / math.max(length, 1e-6))
			table.insert(samples, { p = p })
			want += step
		else
			walked += length
			i += 1
		end
	end
	local at = 0
	for k, s in ipairs(samples) do
		local nextP = samples[k % #samples + 1].p
		local prevP = samples[(k - 2) % #samples + 1].p
		local along = nextP - prevP
		s.t = Vector3.new(along.X, 0, along.Z).Unit
		s.n = s.t:Cross(Vector3.yAxis)
		s.len = (nextP - s.p).Magnitude
		s.at = at
		at += s.len
	end
	samples.length = at
	samples.step = step
	cache[index] = samples
	return samples
end

-- Where the road is `distance` studs round the lap (wrapping), with its
-- direction and right-hand side there.
function Tracks.pointAt(samples, distance)
	local length = samples.length
	distance %= length
	local count = #samples
	local index = math.clamp(math.floor(distance / samples.step) + 1, 1, count)
	while index > 1 and samples[index].at > distance do
		index -= 1
	end
	while index < count and samples[index + 1].at <= distance do
		index += 1
	end
	local s = samples[index]
	local nextS = samples[index % count + 1]
	local f = math.clamp((distance - s.at) / s.len, 0, 1)
	local t = s.t:Lerp(nextS.t, f)
	return s.p:Lerp(nextS.p, f), t.Unit, t.Unit:Cross(Vector3.yAxis), index
end

-- Finds where `position` is along the road, looking near sample `near`:
-- the sample before it, how far on to the next (0 to 1) and how far to the
-- right of the middle it is (negative is left).
function Tracks.locate(samples, position, near)
	local count = #samples
	local best, bestIndex, bestF, bestSide = math.huge, near, 0, 0
	for k = -4, 7 do
		local i = (near + k - 1) % count + 1
		local a = samples[i].p
		local b = samples[i % count + 1].p
		local ab = Vector3.new(b.X - a.X, 0, b.Z - a.Z)
		local ap = Vector3.new(position.X - a.X, 0, position.Z - a.Z)
		local f = math.clamp(ap:Dot(ab) / math.max(ab:Dot(ab), 1e-6), 0, 1)
		local off = ap - ab * f
		local dy = math.abs(position.Y - (a.Y + (b.Y - a.Y) * f))
		local d = off.Magnitude + dy * 0.5
		if d < best then
			best, bestIndex, bestF = d, i, f
			local n = samples[i].n:Lerp(samples[i % count + 1].n, f)
			bestSide = off:Dot(n.Unit)
		end
	end
	return bestIndex, bestF, bestSide
end

-- The same, but searching the whole lap (for a kart that lost its place).
function Tracks.locateAnywhere(samples, position)
	local count = #samples
	local best, near = math.huge, 1
	for i = 1, count do
		local d = (samples[i].p - position).Magnitude
		if d < best then
			best, near = d, i
		end
	end
	return Tracks.locate(samples, position, near)
end

------------------------------------------------------------------------
-- Building.

local function part(parent, props)
	local p = Instance.new("Part")
	p.Anchored = true
	for key, value in props do
		p[key] = value
	end
	p.Parent = parent
	return p
end

-- A block lying from `a` to `b`, `width` wide and `thick` deep, its top on the line.
local function strip(parent, a, b, width, thick, colour, material, name)
	local length = (b - a).Magnitude
	if length < 0.05 then
		return nil
	end
	local up = Vector3.new(0, -thick / 2, 0)
	return part(parent, {
		Name = name or "Road",
		Size = Vector3.new(width, thick, length + 0.6),
		CFrame = CFrame.lookAt((a + b) / 2 + up, b + up),
		Color = colour,
		Material = material or Enum.Material.SmoothPlastic,
	})
end

local function disc(parent, at, width, thick, colour, material, name)
	return part(parent, {
		Name = name or "Road",
		Shape = Enum.PartType.Cylinder,
		Size = Vector3.new(thick, width, width),
		CFrame = CFrame.new(at - Vector3.new(0, thick / 2, 0)) * CFrame.Angles(0, 0, math.pi / 2),
		Color = colour,
		Material = material or Enum.Material.SmoothPlastic,
	})
end

local function random(seed)
	local state = seed
	return function(low, high)
		state = (state * 1103515245 + 12345) % 2147483648
		return low + (high - low) * (state / 2147483648)
	end
end

-- How far `point` is from the road's middle (flat), ignoring the bridge's height.
local function clearance(samples, point)
	local best = math.huge
	for _, s in ipairs(samples) do
		local d = Vector3.new(point.X - s.p.X, 0, point.Z - s.p.Z).Magnitude
		if d < best then
			best = d
		end
	end
	return best
end

local function decorate(folder, track, samples)
	local rnd = random(#track.name * 7919)
	local count = 0
	local tries = 0
	while count < 90 and tries < 600 do
		tries += 1
		local x, z = rnd(-420, 420), rnd(-360, 360)
		local at = Vector3.new(x, 0, z)
		if clearance(samples, at) > Tracks.WALL + 8 then
			count += 1
			local size = rnd(0.8, 1.5)
			if track.decor == "trees" then
				part(folder, { Name = "Trunk", Size = Vector3.new(1.6, 6, 1.6) * size, Position = at + Vector3.new(0, 3 * size, 0), Color = Color3.fromRGB(120, 85, 55), Material = Enum.Material.Wood })
				part(folder, { Name = "Leaves", Shape = Enum.PartType.Ball, Size = Vector3.new(9, 9, 9) * size, Position = at + Vector3.new(0, 8.5 * size, 0), Color = Color3.fromRGB(60 + rnd(0, 30), 140 + rnd(0, 40), 60), Material = Enum.Material.Grass })
			elseif track.decor == "cacti" then
				if rnd(0, 1) < 0.55 then
					part(folder, { Name = "Cactus", Size = Vector3.new(2, 9, 2) * size, Position = at + Vector3.new(0, 4.5 * size, 0), Color = Color3.fromRGB(70, 140, 80), Material = Enum.Material.Grass })
					part(folder, { Name = "Arm", Size = Vector3.new(4, 1.6, 1.6) * size, Position = at + Vector3.new(1.5, 5, 0) * size, Color = Color3.fromRGB(70, 140, 80), Material = Enum.Material.Grass })
					part(folder, { Name = "Arm", Size = Vector3.new(1.6, 3, 1.6) * size, Position = at + Vector3.new(3, 6.5, 0) * size, Color = Color3.fromRGB(70, 140, 80), Material = Enum.Material.Grass })
				else
					part(folder, { Name = "Rock", Size = Vector3.new(rnd(6, 16), rnd(4, 12), rnd(6, 16)), Position = at + Vector3.new(0, 3, 0), Orientation = Vector3.new(0, rnd(0, 90), 0), Color = Color3.fromRGB(176, 110, 72), Material = Enum.Material.Rock })
				end
			else
				part(folder, { Name = "Trunk", Size = Vector3.new(1.4, 4, 1.4) * size, Position = at + Vector3.new(0, 2 * size, 0), Color = Color3.fromRGB(100, 70, 50), Material = Enum.Material.Wood })
				for level = 0, 2 do
					local w = (8 - level * 2.2) * size
					part(folder, { Name = "Needles", Size = Vector3.new(w, 3 * size, w), Position = at + Vector3.new(0, (5 + level * 3) * size, 0), Orientation = Vector3.new(0, level * 30, 0), Color = Color3.fromRGB(50, 110, 80), Material = Enum.Material.Grass })
				end
				part(folder, { Name = "Snowcap", Size = Vector3.new(2, 1.4, 2) * size, Position = at + Vector3.new(0, 11.5 * size, 0), Color = Color3.fromRGB(245, 248, 255), Material = Enum.Material.Snow })
			end
		end
	end
end

-- Builds track `index` into `parent` and returns the folder, with
-- `Boxes`, `Pads` and `Ramps` folders the race script uses.
function Tracks.build(index, parent)
	local track = Tracks.list[index]
	local samples = Tracks.samples(index)
	local folder = Instance.new("Folder")
	folder.Name = "Track"
	local count = #samples
	local half, wall = Tracks.ROAD_HALF, Tracks.WALL

	part(folder, {
		Name = "Ground",
		Size = Vector3.new(1100, 1, 1000),
		Position = Vector3.new(0, -0.5, 0),
		Color = track.ground[1],
		Material = track.ground[2],
	})

	local road = Instance.new("Folder")
	road.Name = "Road"
	road.Parent = folder
	local walls = Instance.new("Folder")
	walls.Name = "Walls"
	walls.Parent = folder
	local lift = Vector3.new(0, 0.2, 0)
	for i, s in ipairs(samples) do
		local nextS = samples[i % count + 1]
		local a, b = s.p + lift, nextS.p + lift
		strip(road, a, b, half * 2, 1, track.road)
		disc(road, a, half * 2, 1, track.road)
		-- Up off the ground, the slow edges need something under them too.
		if s.p.Y > 0.3 or nextS.p.Y > 0.3 then
			strip(road, s.p, nextS.p, wall * 2, 1.2, track.shoulder, track.ground[2], "Edge")
			disc(road, s.p, wall * 2, 1.2, track.shoulder, track.ground[2], "Edge")
			if i % 3 == 0 and s.p.Y > 4 then
				for side = -1, 1, 2 do
					local foot = s.p + s.n * side * (wall - 1)
					local clear = true
					for _, other in ipairs(samples) do
						if other.p.Y < 1 and Vector3.new(other.p.X - foot.X, 0, other.p.Z - foot.Z).Magnitude < wall + 4 then
							clear = false
							break
						end
					end
					if clear then
						part(folder, { Name = "Pillar", Size = Vector3.new(3, s.p.Y, 3), Position = Vector3.new(foot.X, s.p.Y / 2 - 0.6, foot.Z), Color = Color3.fromRGB(150, 140, 130), Material = Enum.Material.Rock })
					end
				end
			end
		end
		-- Kerbs at the road's edge and low walls further out, red and white.
		local kerb = if i % 2 == 0 then Color3.fromRGB(220, 50, 50) else Color3.fromRGB(245, 245, 245)
		for side = -1, 1, 2 do
			local ka = s.p + s.n * side * (half + 1) + Vector3.new(0, 0.28, 0)
			local kb = nextS.p + nextS.n * side * (half + 1) + Vector3.new(0, 0.28, 0)
			strip(road, ka, kb, 2.4, 0.4, kerb, nil, "Kerb")
			local wa = s.p + s.n * side * wall
			local wb = nextS.p + nextS.n * side * wall
			-- On the inside of a tight bend the wall would fold back on itself.
			local along = wb - wa
			if along:Dot(s.t) > 0.3 * s.len then
				local w = strip(walls, wa + Vector3.new(0, 3, 0), wb + Vector3.new(0, 3, 0), 1.6, 3, if i % 2 == 0 then Color3.fromRGB(230, 70, 60) else Color3.fromRGB(240, 240, 240), nil, "Wall")
				if w then
					w.Size += Vector3.new(0, 0, 1)
				end
			end
		end
	end

	-- The start line, chequered, with an arch over it.
	local start = samples[1]
	local line = part(folder, {
		Name = "StartLine",
		Size = Vector3.new(half * 2, 0.1, 3),
		CFrame = CFrame.lookAt(start.p + Vector3.new(0, 0.26, 0), start.p + start.t + Vector3.new(0, 0.26, 0)),
		Color = Color3.fromRGB(255, 255, 255),
		CanCollide = false,
	})
	local checker = Instance.new("Texture")
	checker.Texture = "libroxel://checker"
	checker.Face = Enum.NormalId.Top
	checker.StudsPerTileU = 3
	checker.StudsPerTileV = 3
	checker.Parent = line
	for side = -1, 1, 2 do
		part(folder, { Name = "ArchPost", Size = Vector3.new(1.6, 14, 1.6), Position = start.p + start.n * side * (half + 3) + Vector3.new(0, 7, 0), Color = Color3.fromRGB(40, 40, 48) })
	end
	local banner = part(folder, {
		Name = "ArchBanner",
		Size = Vector3.new(half * 2 + 8, 3, 1),
		CFrame = CFrame.lookAt(start.p + Vector3.new(0, 14, 0), start.p + start.t + Vector3.new(0, 14, 0)),
		Color = Color3.fromRGB(40, 40, 48),
	})
	for _, face in { Enum.NormalId.Front, Enum.NormalId.Back } do
		local sign = Instance.new("SurfaceGui")
		sign.Face = face
		sign.PixelsPerStud = 20
		sign.Parent = banner
		local text = Instance.new("TextLabel")
		text.Size = UDim2.new(1, 0, 1, 0)
		text.BackgroundTransparency = 1
		text.Text = track.name:upper()
		text.TextColor3 = Color3.fromRGB(255, 210, 60)
		text.TextScaled = true
		text.Parent = sign
	end

	-- Item boxes: rows of four across the road.
	local boxes = Instance.new("Folder")
	boxes.Name = "Boxes"
	boxes.Parent = folder
	for _, fraction in track.boxes do
		local p, t, n = Tracks.pointAt(samples, fraction * samples.length)
		for k = -1.5, 1.5 do
			local at = p + n * k * 6.5 + Vector3.new(0, 2.6, 0)
			local box = part(boxes, {
				Name = "ItemBox",
				Size = Vector3.new(3, 3, 3),
				CFrame = CFrame.new(at) * CFrame.Angles(math.rad(35), math.rad(45), 0),
				Color = Color3.fromRGB(120, 200, 255),
				Material = Enum.Material.Neon,
				Transparency = 0.35,
				CanCollide = false,
			})
			local star = Instance.new("Decal")
			star.Texture = "libroxel://star"
			star.Parent = box
		end
	end

	-- Boost pads: glowing arrows flat on the road.
	local pads = Instance.new("Folder")
	pads.Name = "Pads"
	pads.Parent = folder
	for _, pad in track.pads do
		local p, t, n = Tracks.pointAt(samples, pad[1] * samples.length)
		local at = p + n * pad[2] + Vector3.new(0, 0.3, 0)
		local plate = part(pads, {
			Name = "BoostPad",
			Size = Vector3.new(9, 0.2, 12),
			CFrame = CFrame.lookAt(at, at + t),
			Color = Color3.fromRGB(255, 150, 30),
			Material = Enum.Material.Neon,
			CanCollide = false,
		})
		local arrow = Instance.new("Decal")
		arrow.Texture = "libroxel://arrow"
		arrow.Face = Enum.NormalId.Top
		arrow.Parent = plate
	end

	-- Jump ramps: wedges across the road; driving off the top is a jump.
	local ramps = Instance.new("Folder")
	ramps.Name = "Ramps"
	ramps.Parent = folder
	for _, ramp in track.ramps do
		local p, t = Tracks.pointAt(samples, ramp[1] * samples.length)
		local at = p + Vector3.new(0, 2.2, 0)
		-- A wedge is low at its front (-Z) and high at its back, so it faces back down the road.
		part(ramps, {
			Name = "Ramp",
			Shape = Enum.PartType.Wedge,
			Size = Vector3.new(half * 2 - 2, 4, 14),
			CFrame = CFrame.lookAt(at, at - t),
			Color = Color3.fromRGB(250, 200, 60),
			Material = Enum.Material.SmoothPlastic,
		})
	end

	decorate(folder, track, samples)
	folder.Parent = parent
	return folder
end

return Tracks
