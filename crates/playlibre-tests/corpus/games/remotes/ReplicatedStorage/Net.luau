-- Creates and finds remotes by name, the way most games wrap them.
local RunService = game:GetService("RunService")
local Net = {}

local folder = script.Parent:FindFirstChild("Remotes")
if not folder and RunService:IsServer() then
	folder = Instance.new("Folder")
	folder.Name = "Remotes"
	folder.Parent = script.Parent
end

function Net.event(name)
	local remotes = folder or script.Parent:WaitForChild("Remotes")
	local remote = remotes:FindFirstChild(name)
	if not remote and RunService:IsServer() then
		remote = Instance.new("RemoteEvent")
		remote.Name = name
		remote.Parent = remotes
	end
	return remote or remotes:WaitForChild(name)
end

function Net.func(name)
	local remotes = folder or script.Parent:WaitForChild("Remotes")
	local remote = remotes:FindFirstChild(name)
	if not remote and RunService:IsServer() then
		remote = Instance.new("RemoteFunction")
		remote.Name = name
		remote.Parent = remotes
	end
	return remote or remotes:WaitForChild(name)
end

return Net
