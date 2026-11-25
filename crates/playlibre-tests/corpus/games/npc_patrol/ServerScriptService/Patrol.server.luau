-- An NPC that walks between waypoints with Humanoid:MoveTo and
-- PathfindingService, as guard and pet scripts do.
local PathfindingService = game:GetService("PathfindingService")

local function makeNpc()
	local model = Instance.new("Model")
	model.Name = "Guard"
	local root = Instance.new("Part")
	root.Name = "HumanoidRootPart"
	root.Size = Vector3.new(2, 2, 1)
	root.Position = Vector3.new(-20, 3, -20)
	root.Parent = model
	local head = Instance.new("Part")
	head.Name = "Head"
	head.Size = Vector3.new(1, 1, 1)
	head.Position = Vector3.new(-20, 4.5, -20)
	head.Parent = model
	local weld = Instance.new("WeldConstraint")
	weld.Part0 = root
	weld.Part1 = head
	weld.Parent = root
	local humanoid = Instance.new("Humanoid")
	humanoid.WalkSpeed = 16
	humanoid.Parent = model
	model.PrimaryPart = root
	model.Parent = workspace
	return model, humanoid, root
end

local npc, humanoid, root = makeNpc()
print("npc: spawned " .. npc.Name .. " with health " .. humanoid.Health)

local path = PathfindingService:CreatePath({ AgentRadius = 2, AgentHeight = 5, AgentCanJump = true })
local ok, err = pcall(function()
	path:ComputeAsync(root.Position, Vector3.new(-5, 3, -20))
end)
print("npc: path computed", ok, path.Status == Enum.PathStatus.Success)
local waypoints = ok and path:GetWaypoints() or {}
print("npc: has waypoints " .. tostring(#waypoints > 0))

local reached = 0
humanoid.MoveToFinished:Connect(function(done)
	if done then reached += 1 end
end)
for _, target in ipairs({ Vector3.new(-10, 3, -20), Vector3.new(-10, 3, -10) }) do
	humanoid:MoveTo(target)
	humanoid.MoveToFinished:Wait()
end
local off = (root.Position - Vector3.new(-10, root.Position.Y, -10)).Magnitude
print(string.format("npc: reached %d points, within 2 studs %s", reached, tostring(off < 2)))

humanoid:TakeDamage(30)
print("npc: health after hit " .. humanoid.Health)
humanoid.Died:Connect(function()
	print("npc: guard died")
end)
humanoid.Health = 0
