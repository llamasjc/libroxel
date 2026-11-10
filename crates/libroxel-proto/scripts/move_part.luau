--!strict
-- Phase 0 demo: build a tiny scene, then move a part every frame.

local RunService = game:GetService("RunService")

local baseplate = Instance.new("Part")
baseplate.Name = "Baseplate"
baseplate.Size = Vector3.new(32, 1, 32)
baseplate.Position = Vector3.new(0, -0.5, 0)
baseplate.Color = Color3.fromRGB(91, 154, 76)
baseplate.Anchored = true
baseplate.Parent = workspace

local mover = Instance.new("Part")
mover.Name = "Mover"
mover.Size = Vector3.new(2, 2, 2)
mover.Color = Color3.fromRGB(255, 140, 0)
mover.Parent = workspace

local RADIUS = 6
local elapsed = 0

RunService.Heartbeat:Connect(function(dt: number)
	elapsed += dt
	local hop = math.abs(math.sin(elapsed * 3)) * 2
	mover.Position = Vector3.new(math.cos(elapsed) * RADIUS, 1 + hop, math.sin(elapsed) * RADIUS)
end)

print("move_part.luau is running")
