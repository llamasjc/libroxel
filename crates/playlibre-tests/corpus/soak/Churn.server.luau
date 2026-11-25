-- Soak workload: the kinds of churn a server sees over hours. Every wave
-- makes and destroys parts, connects and drops events, starts threads that
-- finish, fills and clears tables, and builds and removes GUI. Nothing here
-- should leave anything behind once a wave is done.
local Players = game:GetService("Players")
local Debris = game:GetService("Debris")
local CollectionService = game:GetService("CollectionService")
local TweenService = game:GetService("TweenService")
local HttpService = game:GetService("HttpService")

local waves = 0
local cache = {}

local function wave()
	local folder = Instance.new("Folder")
	folder.Name = "Wave"
	folder.Parent = workspace
	local connections = {}
	for i = 1, 40 do
		local p = Instance.new("Part")
		p.Size = Vector3.new(1, 1, 1)
		p.Position = Vector3.new((i % 8) * 3, 10 + i, (i // 8) * 3)
		p.Anchored = i % 2 == 0
		p:SetAttribute("Wave", waves)
		CollectionService:AddTag(p, "Churn")
		p.Parent = folder
		table.insert(connections, p.Touched:Connect(function() end))
		if i % 10 == 0 then
			TweenService:Create(p, TweenInfo.new(0.2), { Transparency = 1 }):Play()
		end
		if i % 7 == 0 then
			Debris:AddItem(p, 0.5)
		end
	end
	for _ = 1, 10 do
		task.spawn(function()
			task.wait(math.random() * 0.5)
		end)
	end
	local blob = {}
	for i = 1, 200 do
		blob[i] = { id = i, name = "item" .. i }
	end
	cache[waves % 5] = HttpService:JSONEncode(blob)
	for _, player in ipairs(Players:GetPlayers()) do
		local gui = player:FindFirstChildOfClass("PlayerGui")
		if gui then
			local screen = Instance.new("ScreenGui")
			local label = Instance.new("TextLabel")
			label.Text = "wave " .. waves
			label.Parent = screen
			screen.Parent = gui
			Debris:AddItem(screen, 0.5)
		end
	end
	task.wait(0.8)
	for _, c in ipairs(connections) do
		c:Disconnect()
	end
	folder:Destroy()
	waves += 1
	if waves % 50 == 0 then
		print("churn: " .. waves .. " waves")
	end
end

while true do
	wave()
end
