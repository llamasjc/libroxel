-- Arena: the pieces every round-based game here shares. It keeps the
-- status line and round clock that the HUD shows, fills empty places
-- with computer players, moves everyone about, and keeps the scores.
--
-- A "contestant" is anyone taking part, a player or a bot, as a table:
--   { name, model, humanoid, root, player (or nil), bot (true or nil) }

local Players = game:GetService("Players")
local ServerStorage = game:GetService("ServerStorage")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local PathfindingService = game:GetService("PathfindingService")

local Arena = {}

local BOT_NAMES = {
	"Pip", "Mochi", "Zed", "Juno", "Biscuit", "Nova", "Rex", "Tilly",
	"Ollie", "Kiwi", "Ziggy", "Luna", "Bolt", "Maple", "Echo", "Pixel",
}
local SHIRTS = {
	Color3.fromRGB(230, 80, 70), Color3.fromRGB(70, 130, 230), Color3.fromRGB(250, 190, 50),
	Color3.fromRGB(80, 190, 100), Color3.fromRGB(170, 90, 210), Color3.fromRGB(240, 140, 60),
	Color3.fromRGB(60, 190, 200), Color3.fromRGB(235, 110, 170),
}

local folder = Instance.new("Folder")
folder.Name = "Round"
folder.Parent = ReplicatedStorage

-- What the HUD shows: a line of text, and the seconds left (-1 hides it).
local status = Instance.new("StringValue")
status.Name = "Status"
status.Parent = folder
local clock = Instance.new("IntValue")
clock.Name = "Clock"
clock.Value = -1
clock.Parent = folder
-- Short messages that pop up in the middle of the screen.
local notify = Instance.new("RemoteEvent")
notify.Name = "Notify"
notify.Parent = folder

local bots = Instance.new("Folder")
bots.Name = "Bots"
bots.Parent = workspace

function Arena.status(text)
	status.Value = text
end

function Arena.clock(seconds)
	clock.Value = seconds and math.max(0, math.ceil(seconds)) or -1
end

-- A message for everyone, or for one player.
function Arena.tell(text, player)
	if player then
		notify:FireClient(player, text)
	else
		notify:FireAllClients(text)
	end
end

-- Sets a player's role, which the HUD shows in a coloured badge.
function Arena.role(c, role)
	if c.player then
		c.player:SetAttribute("Role", role)
	end
	c.model:SetAttribute("Role", role)
end

------------------------------------------------------------------------
-- Scores.

local function stats(player)
	local board = player:FindFirstChild("leaderstats")
	if not board then
		board = Instance.new("Folder")
		board.Name = "leaderstats"
		board.Parent = player
	end
	return board
end

-- Adds a score to the leaderboard (Wins first, as it's made first).
function Arena.score(name)
	local function add(player)
		local value = Instance.new("IntValue")
		value.Name = name
		value.Parent = stats(player)
	end
	Players.PlayerAdded:Connect(add)
	for _, player in Players:GetPlayers() do
		add(player)
	end
end

function Arena.bump(c, name, by)
	if c.player then
		local value = stats(c.player):FindFirstChild(name)
		if value then
			value.Value += by or 1
		end
	end
end

------------------------------------------------------------------------
-- Who's playing.

local function alive(model)
	local humanoid = model and model:FindFirstChildOfClass("Humanoid")
	local root = model and model:FindFirstChild("HumanoidRootPart")
	if humanoid and root and humanoid.Health > 0 then
		return humanoid, root
	end
	return nil
end

-- Everyone who could take part right now: every player with a living
-- character, then every bot.
function Arena.everyone()
	local list = {}
	for _, player in Players:GetPlayers() do
		local humanoid, root = alive(player.Character)
		if humanoid then
			table.insert(list, {
				name = player.DisplayName ~= "" and player.DisplayName or player.Name,
				model = player.Character,
				humanoid = humanoid,
				root = root,
				player = player,
			})
		end
	end
	for _, model in bots:GetChildren() do
		local humanoid, root = alive(model)
		if humanoid then
			table.insert(list, { name = model.Name, model = model, humanoid = humanoid, root = root, bot = true })
		end
	end
	return list
end

-- Whether a contestant is still in the game (still here and alive).
function Arena.inGame(c)
	if not c.model.Parent or c.humanoid.Health <= 0 then
		return false
	end
	if c.player then
		return c.player.Parent ~= nil and c.player.Character == c.model
	end
	return true
end

-- Adds bots until there are `total` contestants, or takes some away when
-- people have joined. Bots stand at `where` (a Vector3) until placed.
function Arena.fillWithBots(total, where)
	local wanted = math.max(0, total - #Players:GetPlayers())
	local have = bots:GetChildren()
	for i = wanted + 1, #have do
		have[i]:Destroy()
	end
	local taken = {}
	for _, model in bots:GetChildren() do
		taken[model.Name] = true
	end
	local template = ServerStorage:FindFirstChild("Bot")
	if not template then
		return
	end
	for i = #bots:GetChildren() + 1, wanted do
		local name
		repeat
			name = BOT_NAMES[math.random(#BOT_NAMES)]
		until not taken[name]
		taken[name] = true
		local model = template:Clone()
		model.Name = name
		local torso = model:FindFirstChild("Torso")
		if torso then
			torso.Color = SHIRTS[(i - 1) % #SHIRTS + 1]
		end
		model:PivotTo(CFrame.new(where + Vector3.new(math.random(-6, 6), 0, math.random(-6, 6))))
		model.Parent = bots
	end
end

-- Bots that died are taken away; the next round brings new ones.
function Arena.clearDeadBots()
	for _, model in bots:GetChildren() do
		local humanoid = model:FindFirstChildOfClass("Humanoid")
		if not humanoid or humanoid.Health <= 0 then
			model:Destroy()
		end
	end
end

function Arena.removeBots()
	bots:ClearAllChildren()
end

------------------------------------------------------------------------
-- Moving people about.

-- Puts a contestant at `position` (feet on the ground there).
function Arena.place(c, position)
	if c.bot then
		c.humanoid:MoveTo(position)
	end
	c.model:PivotTo(CFrame.new(position + Vector3.new(0, 3, 0)))
end

function Arena.speed(c, speed)
	c.humanoid.WalkSpeed = speed
end

-- Sends a bot walking straight to `position` (to chase someone it can see).
local routes = {} -- by bot model: the route it's following
function Arena.walk(c, position)
	if c.bot then
		routes[c.model] = nil
		c.humanoid:MoveTo(position)
	end
end

local FLAT = Vector3.new(1, 0, 1)

-- Sends a bot to `goal` the long way round: through doors and round
-- walls, along a path from the pathfinding service.
function Arena.go(c, goal)
	if not c.bot then
		return
	end
	local route = routes[c.model]
	if route and (route.goal - goal).Magnitude < 4 and time() - route.at < 6 then
		return
	end
	route = { goal = goal, at = time() }
	routes[c.model] = route
	task.spawn(function()
		local path = PathfindingService:CreatePath({ AgentRadius = 1, AgentHeight = 5, AgentCanJump = true })
		local ok = pcall(function()
			path:ComputeAsync(c.root.Position, goal)
		end)
		if routes[c.model] ~= route then
			return
		end
		if not ok or path.Status ~= Enum.PathStatus.Success then
			c.humanoid:MoveTo(goal)
			return
		end
		for _, point in path:GetWaypoints() do
			if routes[c.model] ~= route or not Arena.inGame(c) then
				return
			end
			if point.Action == Enum.PathWaypointAction.Jump then
				c.humanoid.Jump = true
			end
			c.humanoid:MoveTo(point.Position)
			local giveUp = time() + 2
			while time() < giveUp and ((c.root.Position - point.Position) * FLAT).Magnitude > 2 do
				task.wait(0.1)
				if routes[c.model] ~= route then
					return
				end
			end
		end
	end)
end

function Arena.mark(c, colour)
	local glow = c.model:FindFirstChild("RoundGlow")
	if not colour then
		if glow then
			glow:Destroy()
		end
		return
	end
	if not glow then
		glow = Instance.new("Highlight")
		glow.Name = "RoundGlow"
		glow.FillTransparency = 0.5
		glow.Parent = c.model
	end
	glow.FillColor = colour
	glow.OutlineColor = colour
end

-- Puts everything a round changed back: speed, glow and role.
function Arena.reset(c)
	Arena.speed(c, 16)
	Arena.mark(c, nil)
	Arena.role(c, "")
	if c.player then
		c.player:SetAttribute("Blind", false)
	end
end

function Arena.distance(a, b)
	local offset = a.root.Position - b.root.Position
	return Vector3.new(offset.X, 0, offset.Z).Magnitude, math.abs(offset.Y)
end

-- Whether `a` can see `b`: nothing solid between their heads.
function Arena.sees(a, b)
	local from = a.root.Position + Vector3.new(0, 1.5, 0)
	local to = b.root.Position + Vector3.new(0, 1.5, 0)
	local params = RaycastParams.new()
	params.FilterType = Enum.RaycastFilterType.Exclude
	params.FilterDescendantsInstances = { a.model, b.model, bots }
	local hit = workspace:Raycast(from, to - from, params)
	if hit and hit.Instance and hit.Instance.CanCollide and hit.Instance.Transparency < 0.6 then
		-- Something solid is in the way, unless it's another person.
		local owner = hit.Instance:FindFirstAncestorOfClass("Model")
		return owner ~= nil and owner:FindFirstChildOfClass("Humanoid") ~= nil
	end
	return true
end

-- The children of a folder of marker parts, as positions on the ground.
function Arena.spots(markers)
	local list = {}
	for _, part in markers:GetChildren() do
		if part:IsA("BasePart") then
			table.insert(list, part.Position)
		end
	end
	return list
end

function Arena.shuffle(list)
	for i = #list, 2, -1 do
		local j = math.random(i)
		list[i], list[j] = list[j], list[i]
	end
	return list
end

-- Waits `seconds`, counting down on the clock. Stops early (returning
-- true) if `done` says so.
function Arena.countdown(seconds, done, every)
	local finish = time() + seconds
	while time() < finish do
		Arena.clock(finish - time())
		if done and done() then
			return true
		end
		task.wait(every or 0.1)
	end
	Arena.clock(0)
	return false
end

return Arena
