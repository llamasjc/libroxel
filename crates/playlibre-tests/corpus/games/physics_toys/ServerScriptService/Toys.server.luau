-- Loose parts, welded assemblies, explosions and velocity movers: the
-- physics a sandbox game leans on.
local crate = Instance.new("Part")
crate.Name = "Crate"
crate.Size = Vector3.new(2, 2, 2)
crate.Position = Vector3.new(20, 20, 0)
crate.Parent = workspace

local car = Instance.new("Model")
car.Name = "Cart"
local body = Instance.new("Part")
body.Size = Vector3.new(4, 1, 6)
body.Position = Vector3.new(-20, 3, 0)
body.Parent = car
for i, offset in ipairs({ Vector3.new(-2, -1, -2), Vector3.new(2, -1, -2), Vector3.new(-2, -1, 2), Vector3.new(2, -1, 2) }) do
	local wheel = Instance.new("Part")
	wheel.Name = "Wheel" .. i
	wheel.Shape = Enum.PartType.Ball
	wheel.Size = Vector3.new(1, 1, 1)
	wheel.Position = body.Position + offset
	wheel.Parent = car
	local weld = Instance.new("WeldConstraint")
	weld.Part0 = body
	weld.Part1 = wheel
	weld.Parent = body
end
car.PrimaryPart = body
car.Parent = workspace

local attachment = Instance.new("Attachment")
attachment.Parent = body
local mover = Instance.new("LinearVelocity")
mover.Attachment0 = attachment
mover.VectorVelocity = Vector3.new(0, 0, 10)
mover.MaxForce = math.huge
mover.Parent = body

task.wait(2)
print(string.format("physics: crate fell %s", tostring(crate.Position.Y < 5)))
print(string.format("physics: cart moved %s", tostring(body.Position.Z > 3)))
local wheel = car:FindFirstChild("Wheel1")
print(string.format("physics: wheel kept up %s", tostring((wheel.Position - body.Position).Magnitude < 4)))

local blast = Instance.new("Explosion")
blast.Position = crate.Position
blast.BlastRadius = 8
blast.BlastPressure = 500000
local hits = 0
blast.Hit:Connect(function(part)
	if part == crate then hits += 1 end
end)
blast.Parent = workspace
task.wait(0.5)
print("physics: explosion hit crate " .. tostring(hits > 0))

local _, size = car:GetBoundingBox()
print(string.format("physics: cart box %.0f wide", size.X))
crate:Destroy()
print("physics: crate gone " .. tostring(crate.Parent == nil))
