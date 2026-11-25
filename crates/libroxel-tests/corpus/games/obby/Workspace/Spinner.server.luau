-- A spinning bar obstacle driven by Heartbeat, as most obbies do it.
local RunService = game:GetService("RunService")

local bar = Instance.new("Part")
bar.Name = "Spinner"
bar.Anchored = true
bar.Size = Vector3.new(20, 1, 1)
bar.Position = Vector3.new(30, 3, 15)
bar.Parent = workspace

local angle = 0
local turns = 0
RunService.Heartbeat:Connect(function(dt)
	angle += dt * math.pi
	if angle >= math.pi * 2 then
		angle -= math.pi * 2
		turns += 1
		if turns == 1 then
			print("obby: spinner turned once")
		end
	end
	bar.CFrame = CFrame.new(bar.Position) * CFrame.Angles(0, angle, 0)
end)
