-- The CFrame and vector maths cameras, guns and building tools rely on.
local function near(a, b) return math.abs(a - b) < 1e-3 end
local function v(x) return string.format("%.2f", x) end

local origin = CFrame.new(0, 5, 0)
local look = CFrame.lookAt(Vector3.new(0, 0, 0), Vector3.new(0, 0, -10))
print("cframe: lookAt forward z " .. v(look.LookVector.Z))

local rotated = origin * CFrame.Angles(0, math.rad(90), 0)
local ahead = rotated * Vector3.new(0, 0, -5)
print("cframe: rotated point " .. v(ahead.X) .. " " .. v(ahead.Y) .. " " .. v(ahead.Z))

local rel = rotated:ToObjectSpace(CFrame.new(3, 5, 0))
local back = rotated:ToWorldSpace(rel)
print("cframe: round trip " .. tostring(near(back.Position.X, 3) and near(back.Position.Y, 5)))
print("cframe: inverse " .. tostring(near((rotated * rotated:Inverse()).Position.Magnitude, 0)))

local a, b = CFrame.new(0, 0, 0), CFrame.new(10, 0, 0)
print("cframe: lerp " .. v(a:Lerp(b, 0.25).X))
local rx, ry, rz = CFrame.Angles(0.1, 0.2, 0.3):ToEulerAnglesXYZ()
print("cframe: euler " .. v(rx) .. " " .. v(ry) .. " " .. v(rz))
print("cframe: components " .. select("#", CFrame.new(1, 2, 3):GetComponents()))

local vec = Vector3.new(3, 4, 0)
print("vector: magnitude " .. vec.Magnitude .. " unit " .. v(vec.Unit.X))
print("vector: dot " .. vec:Dot(Vector3.xAxis) .. " cross z " .. Vector3.xAxis:Cross(Vector3.yAxis).Z)
print("vector: lerp " .. tostring(Vector3.zero:Lerp(Vector3.one, 0.5)))
print("vector: min " .. tostring(vec:Min(Vector3.new(1, 10, 1))))
print("vector: fuzzy " .. tostring(vec:FuzzyEq(Vector3.new(3, 4, 0.00001), 0.001)))
local key = "X"
print("vector: dynamic key " .. vec[key])

local v2 = Vector2.new(6, 8)
print("vector2: " .. v2.Magnitude .. " " .. tostring(v2 * 0.5))
local u = UDim2.new(0.5, 10, 1, -20) + UDim2.fromOffset(5, 5)
print("udim2: " .. u.X.Scale .. " " .. u.X.Offset .. " " .. u.Y.Offset)

local c = Color3.fromRGB(255, 128, 0)
local h, s, val = c:ToHSV()
print("color: hsv " .. v(h) .. " " .. v(s) .. " " .. v(val) .. " hex " .. c:ToHex())
print("color: lerp " .. v(Color3.new(0, 0, 0):Lerp(Color3.new(1, 1, 1), 0.5).R))

-- Raycasting against a part, with filter params.
local wall = Instance.new("Part")
wall.Anchored = true
wall.Size = Vector3.new(10, 10, 1)
wall.Position = Vector3.new(0, 5, -30)
wall.Parent = workspace
local params = RaycastParams.new()
params.FilterType = Enum.RaycastFilterType.Include
params.FilterDescendantsInstances = { wall }
local result = workspace:Raycast(Vector3.new(0, 5, 0), Vector3.new(0, 0, -100), params)
print("raycast: hit " .. tostring(result and result.Instance == wall) .. " at z " .. (result and v(result.Position.Z) or "none"))
params.FilterType = Enum.RaycastFilterType.Exclude
local miss = workspace:Raycast(Vector3.new(0, 5, 0), Vector3.new(0, 0, -100), params)
print("raycast: excluded " .. tostring(miss == nil or miss.Instance ~= wall))
