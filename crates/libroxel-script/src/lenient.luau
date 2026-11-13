-- The lenient layer: members that scripts written for other engines reach
-- for and that libroxel has no engine side for yet (or only part of one).
--
-- Reading one gives a sensible default, setting one remembers the value
-- (while the game runs; it isn't saved), events can be connected to (some
-- fire, see `signals`), and methods do the nearest thing libroxel can. The
-- bindings only come here once the real members, properties and children
-- have all said no, so nothing here hides a real member.
--
-- Like names.rs, this file is script-facing names in one place. It is
-- written from the public descriptions of the API style only.

local Enum, g = ...

local L = {}
local NIL = {} -- a default of nil
local store = {} -- per object: what scripts set, and its Lua-made events
-- The object a method is running on, for methods that remember things.
-- Methods read it before they first wait.
local CURRENT

local function slot(key)
	local s = store[key]
	if not s then
		s = { values = {}, signals = {} }
		store[key] = s
	end
	return s
end

------------------------------------------------------------------------------
-- Events made in Lua
------------------------------------------------------------------------------

local Signal = {}
Signal.__index = Signal
Signal.__tostring = function()
	return "Signal"
end

local function new_signal()
	return setmetatable({ handlers = {} }, Signal)
end

function Signal:Connect(fn)
	local conn = { Connected = true }
	local handler = { fn = fn, conn = conn }
	table.insert(self.handlers, handler)
	conn.Disconnect = function()
		conn.Connected = false
		local i = table.find(self.handlers, handler)
		if i then
			table.remove(self.handlers, i)
		end
	end
	return conn
end
Signal.ConnectParallel = Signal.Connect

function Signal:Once(fn)
	local conn
	conn = self:Connect(function(...)
		conn:Disconnect()
		fn(...)
	end)
	return conn
end

function Signal:Wait()
	local result
	self:Once(function(...)
		result = table.pack(...)
	end)
	while not result do
		task.wait()
	end
	return table.unpack(result, 1, result.n)
end

function Signal:Fire(...)
	for _, h in ipairs(table.clone(self.handlers)) do
		if h.conn.Connected then
			task.spawn(h.fn, ...)
		end
	end
end

L.new_signal = new_signal

------------------------------------------------------------------------------
-- Enumerations libroxel doesn't have
------------------------------------------------------------------------------

local ItemMeta = {}
ItemMeta.__index = ItemMeta
ItemMeta.__tostring = function(item)
	return "Enum." .. tostring(item.EnumType) .. "." .. item.Name
end
function ItemMeta:IsA(name)
	return tostring(self.EnumType) == name
end

local function make_enum(name, items)
	local by_name, list = {}, {}
	local enum = setmetatable({}, {
		__index = function(_, k)
			local item = by_name[k]
			if item == nil then
				error(tostring(k) .. " is not a valid member of " .. name, 0)
			end
			return item
		end,
		__tostring = function()
			return name
		end,
	})
	local value = 0
	for _, item in ipairs(items) do
		local n = item
		if type(item) == "table" then
			n, value = item[1], item[2]
		end
		local it = setmetatable({ Name = n, Value = value, EnumType = enum }, ItemMeta)
		by_name[n] = it
		table.insert(list, it)
		value += 1
	end
	by_name.GetEnumItems = function()
		return table.clone(list)
	end
	by_name.FromName = function(_, n)
		return rawget(by_name, n)
	end
	by_name.FromValue = function(_, v)
		for _, it in ipairs(list) do
			if it.Value == v then
				return it
			end
		end
		return nil
	end
	return enum
end

local ENUMS = {
	HumanoidStateType = {
		{ "FallingDown", 0 }, { "Ragdoll", 1 }, { "GettingUp", 2 }, { "Jumping", 3 }, { "Swimming", 4 },
		{ "Freefall", 5 }, { "Flying", 6 }, { "Landed", 7 }, { "Running", 8 }, { "RunningNoPhysics", 10 },
		{ "StrafingNoPhysics", 11 }, { "Climbing", 12 }, { "Seated", 13 }, { "PlatformStanding", 14 },
		{ "Dead", 15 }, { "Physics", 16 }, { "None", 18 },
	},
	RenderPriority = { { "First", 0 }, { "Input", 100 }, { "Camera", 200 }, { "Character", 300 }, { "Last", 2000 } },
	InputType = { "NoInput", "Constant", "Sin" },
	SurfaceType = { "Smooth", "Glue", "Weld", "Studs", "Inlet", "Universal", "Hinge", "Motor", "SteppingMotor", "SmoothNoOutlines" },
	Font = {
		"Legacy", "Arial", "ArialBold", "SourceSans", "SourceSansBold", "SourceSansLight", "SourceSansItalic",
		"Bodoni", "Garamond", "Cartoon", "Code", "Highway", "SciFi", "Arcade", "Fantasy", "Antique",
		"SourceSansSemibold", "Gotham", "GothamMedium", "GothamBold", "GothamBlack", "AmaticSC", "Bangers",
		"Creepster", "DenkOne", "Fondamento", "FredokaOne", "GrenzeGotisch", "IndieFlower", "JosefinSans", "Jura",
		"Kalam", "LuckiestGuy", "Merriweather", "Michroma", "Nunito", "Oswald", "PatrickHand", "PermanentMarker",
		"Roboto", "RobotoCondensed", "RobotoMono", "Sarpanch", "SpecialElite", "TitilliumWeb", "Ubuntu", "Arimo",
		"ArimoBold", { "Unknown", 100 },
	},
	FontWeight = {
		{ "Thin", 100 }, { "ExtraLight", 200 }, { "Light", 300 }, { "Regular", 400 }, { "Medium", 500 },
		{ "SemiBold", 600 }, { "Bold", 700 }, { "ExtraBold", 800 }, { "Heavy", 900 },
	},
	FontStyle = { "Normal", "Italic" },
	SortOrder = { "Name", "Custom", "LayoutOrder" },
	HorizontalAlignment = { "Center", "Left", "Right" },
	VerticalAlignment = { "Center", "Top", "Bottom" },
	AnimationPriority = { "Idle", "Movement", "Action", "Action2", "Action3", "Action4", { "Core", 1000 } },
	CoreGuiType = { "PlayerList", "Health", "Backpack", "Chat", "All", "EmotesMenu", "SelfView", "Captures" },
	ThumbnailType = { "HeadShot", "AvatarBust", "AvatarThumbnail" },
	ThumbnailSize = { "Size48x48", "Size180x180", "Size420x420", "Size60x60", "Size100x100", "Size150x150", "Size352x352" },
	PlaybackState = { "Begin", "Delayed", "Playing", "Paused", "Completed", "Cancelled" },
	TweenStatus = { "Canceled", "Completed" },
	ContextActionResult = { "Sink", "Pass" },
	MouseBehavior = { "Default", "LockCenter", "LockCurrentPosition" },
	AutomaticSize = { "None", "X", "Y", "XY" },
	HumanoidDisplayDistanceType = { "Viewer", "Subject", "None" },
	HumanoidHealthDisplayType = { "DisplayWhenDamaged", "AlwaysOn", "AlwaysOff" },
	HumanoidRigType = { "R6", "R15" },
	TextTruncate = { "None", "AtEnd", "SplitWord" },
	ZIndexBehavior = { "Global", "Sibling" },
	SizeConstraint = { "RelativeXY", "RelativeXX", "RelativeYY" },
	BorderMode = { "Outline", "Middle", "Inset" },
	ScrollingDirection = { { "X", 1 }, { "Y", 2 }, { "XY", 4 } },
	ElasticBehavior = { "WhenScrollable", "Always", "Never" },
	ScrollBarInset = { "None", "ScrollBar", "Always" },
	ScreenInsets = { "None", "DeviceSafeInsets", "CoreUISafeInsets", "TopbarSafeInsets" },
	CollisionFidelity = { "Default", "Hull", "Box", "PreciseConvexDecomposition" },
	RenderFidelity = { "Automatic", "Precise", "Performance" },
	MeshType = { "Head", "Torso", "Wedge", "Sphere", "Cylinder", "FileMesh", "Brick", "Prism", "Pyramid", "ParallelRamp", "RightAngleRamp", "CornerWedge" },
	CameraMode = { "Classic", "LockFirstPerson" },
	Technology = { "Legacy", "Voxel", "Compatibility", "ShadowMap", "Future" },
	ProximityPromptStyle = { "Default", "Custom" },
	ProximityPromptExclusivity = { "OnePerButton", "OneGlobally", "AlwaysShow" },
	ProximityPromptInputType = { "Keyboard", "Gamepad", "Touch" },
	InfoType = { { "Asset", 1 }, { "Product", 2 }, { "GamePass", 3 }, { "Subscription", 4 }, { "Bundle", 5 } },
	ProductPurchaseDecision = { "NotProcessedYet", "PurchaseGranted" },
	MembershipType = { "None", { "Premium", 4 } },
	SortDirection = { "Ascending", "Descending" },
	DevComputerMovementMode = { "UserChoice", "KeyboardMouse", "ClickToMove", "Scriptable" },
	DevTouchMovementMode = { "UserChoice", "Thumbstick", "DPad", "Thumbpad", "ClickToMove", "Scriptable", "DynamicThumbstick" },
	ComputerMovementMode = { "Default", "KeyboardMouse", "ClickToMove" },
	DevCameraOcclusionMode = { "Zoom", "Invisicam" },
	RollOffMode = { "Inverse", "Linear", "LinearSquare", "InverseTapered" },
	Platform = { "Windows", "OSX", "IOS", "Android", "XBoxOne", "PS4", "PS3", "XBox360", "WiiU", "NX", "Ouya", "AndroidTV", "Chromecast", "Linux", "SteamOS", "WebOS", "DOS", "BeOS", "UWP", "None" },
	TeleportType = { "ToPlace", "ToInstance", "ToReservedServer" },
	StartCorner = { "TopLeft", "TopRight", "BottomLeft", "BottomRight" },
	TextDirection = { "Auto", "LeftToRight", "RightToLeft" },
	ResamplerMode = { "Default", "Pixelated" },
	ApplyStrokeMode = { "Contextual", "Border" },
	LineJoinMode = { "Round", "Bevel", "Miter" },
	HighlightDepthMode = { "AlwaysOnTop", "Occluded" },
	ModelStreamingMode = { "Default", "Atomic", "Persistent", "PersistentPerPlayer", "Nonatomic" },
	ModelLevelOfDetail = { "Automatic", "StreamingMesh", "Disabled" },
	ChatVersion = { "LegacyChatService", "TextChatService" },
	HumanoidCollisionType = { "OuterBox", "InnerBox" },
	AlignType = { "Parallel", "Perpendicular" },
	ActuatorRelativeTo = { "Attachment0", "Attachment1", "World" },
	VelocityConstraintMode = { "Line", "Plane", "Vector" },
	SurfaceGuiSizingMode = { "FixedSize", "PixelsPerStud" },
	Limb = { "Head", "Torso", "LeftArm", "RightArm", "LeftLeg", "RightLeg", "Unknown" },
	AccessoryType = { "Unknown", "Hat", "Hair", "Face", "Neck", "Shoulder", "Front", "Back", "Waist" },
	ConnectionState = { "Connected", "Disconnected" },
	DataStoreRequestType = { "GetAsync", "SetIncrementAsync", "UpdateAsync", "GetSortedAsync", "SetIncrementSortedAsync", "OnUpdate" },
	MessageType = { "MessageOutput", "MessageInfo", "MessageWarning", "MessageError" },
	KeyInterpolationMode = { "Constant", "Linear", "Cubic" },
	PoseEasingStyle = { "Linear", "Constant", "Elastic", "Cubic", "Bounce", "CubicV2" },
	PoseEasingDirection = { "In", "Out", "InOut" },
	FilterResult = { "Accepted", "Rejected" },
	ChatPrivacyMode = { "AllUsers", "NoOne", "Friends" },
	UserCFrame = { "Head", "LeftHand", "RightHand", "Floor" },
	DeviceType = { "Unknown", "Desktop", "Tablet", "Phone" },
	FriendStatus = { "Unknown", "NotFriend", "Friend", "FriendRequestSent", "FriendRequestReceived" },
	ExplosionType = { "NoCraters", "Craters" },
	ParticleOrientation = { "FacingCamera", "FacingCameraWorldUp", "VelocityParallel", "VelocityPerpendicular" },
	TextureMode = { "Stretch", "Wrap", "Static" },
}

function L.add_enums(lib)
	for name, items in pairs(ENUMS) do
		if rawget(lib, name) == nil then
			rawset(lib, name, make_enum(name, items))
		end
	end
end

------------------------------------------------------------------------------
-- Helpers
------------------------------------------------------------------------------

local function service(name)
	return game:GetService(name)
end

local function root_part(model)
	return model and model:FindFirstChild("HumanoidRootPart")
end

local function velocity(part)
	local ok, v = pcall(function()
		return part.AssemblyLinearVelocity
	end)
	return ok and v or Vector3.zero
end

local function viewport()
	local camera = workspace.CurrentCamera
	return camera and camera.ViewportSize or Vector2.new(1280, 720)
end

-- A GUI object's size in pixels, worked out from its UDim2 and its parent's.
local function abs_size(obj)
	local ok, size = pcall(function()
		return obj.Size
	end)
	local parent = obj.Parent
	local within
	if parent and parent:IsA("GuiObject") then
		within = abs_size(parent)
	else
		within = viewport()
	end
	if not ok or typeof(size) ~= "UDim2" then
		return within
	end
	return Vector2.new(size.X.Scale * within.X + size.X.Offset, size.Y.Scale * within.Y + size.Y.Offset)
end

local function abs_position(obj)
	local parent = obj.Parent
	local origin, within = Vector2.zero, viewport()
	if parent and parent:IsA("GuiObject") then
		origin, within = abs_position(parent), abs_size(parent)
	end
	local pos = obj.Position
	local anchor = obj.AnchorPoint
	local size = abs_size(obj)
	return origin
		+ Vector2.new(pos.X.Scale * within.X + pos.X.Offset, pos.Y.Scale * within.Y + pos.Y.Offset)
		- Vector2.new(anchor.X * size.X, anchor.Y * size.Y)
end

local function easing(style, default)
	if style == nil then
		return default
	end
	if typeof(style) == "string" then
		return Enum.EasingStyle[style]
	end
	return style
end

local function tween_gui(obj, goal, direction, style, time, override, callback)
	local info = TweenInfo.new(
		time or 1,
		easing(style, Enum.EasingStyle.Quad),
		typeof(direction) == "string" and Enum.EasingDirection[direction] or direction or Enum.EasingDirection.Out
	)
	local tween = service("TweenService"):Create(obj, info, goal)
	tween:Play()
	if callback then
		task.delay(time or 1, callback, Enum.TweenStatus.Completed)
	end
	return true
end

-- A ray from the camera through a pixel on the screen.
local function screen_ray(camera, x, y, depth)
	local size = camera.ViewportSize
	local cf = camera.CFrame
	local tan = math.tan(math.rad(camera.FieldOfView) / 2)
	local aspect = size.X / math.max(size.Y, 1)
	local nx = (x / math.max(size.X, 1)) * 2 - 1
	local ny = 1 - (y / math.max(size.Y, 1)) * 2
	local dir = (cf.LookVector + cf.RightVector * (nx * tan * aspect) + cf.UpVector * (ny * tan)).Unit
	return Ray.new(cf.Position + dir * (depth or 0), dir)
end

-- Sweeps a shape by stepping it along the direction and checking for
-- overlaps (or, for a sphere, by casting a few rays around it).
local function sweep(cf, size, direction, params)
	local length = direction.Magnitude
	if length < 1e-6 then
		return nil
	end
	local unit = direction / length
	local hit = workspace:Raycast(cf.Position, direction, params)
	local steps = math.clamp(math.ceil(length / math.max(math.min(size.X, size.Y, size.Z), 0.5)), 1, 64)
	for i = 1, steps do
		local at = cf + unit * (length * i / steps)
		local parts = workspace:GetPartBoundsInBox(at, size, nil)
		local filter = params and params.FilterDescendantsInstances or {}
		local exclude = not params or params.FilterType == Enum.RaycastFilterType.Exclude
		for _, part in ipairs(parts) do
			local listed = false
			for _, f in ipairs(filter) do
				if part == f or part:IsDescendantOf(f) then
					listed = true
					break
				end
			end
			if listed ~= exclude then
				local distance = length * i / steps
				if hit and hit.Distance <= distance then
					return hit
				end
				return {
					Instance = part,
					Position = at.Position,
					Normal = -unit,
					Distance = distance,
					Material = part.Material,
				}
			end
		end
	end
	return hit
end

------------------------------------------------------------------------------
-- Humanoid states
------------------------------------------------------------------------------

local State -- Enum.HumanoidStateType, once the enums are in

local function humanoid_state(h, s)
	if s.values.__forced and s.values.__forced_until and time() < s.values.__forced_until then
		return s.values.__forced
	end
	if h.Health <= 0 then
		return State.Dead
	end
	if h.Sit then
		return State.Seated
	end
	local root = root_part(h.Parent)
	if root then
		local v = velocity(root)
		if v.Y > 2 then
			return State.Jumping
		elseif v.Y < -2 then
			return State.Freefall
		end
	end
	return State.Running
end

-- Watches a humanoid each frame so its state events fire.
local function watch_humanoid(h, s)
	if s.watching then
		return
	end
	s.watching = true
	local last, last_speed = humanoid_state(h, s), -1
	local conn
	conn = service("RunService").Heartbeat:Connect(function()
		if not h.Parent then
			conn:Disconnect()
			return
		end
		local now = humanoid_state(h, s)
		local sig = s.signals
		if now ~= last then
			if sig.StateChanged then
				sig.StateChanged:Fire(last, now)
			end
			if now == State.Jumping and sig.Jumping then
				sig.Jumping:Fire(true)
			elseif last == State.Jumping and sig.Jumping then
				sig.Jumping:Fire(false)
			end
			if now == State.Freefall and sig.FreeFalling then
				sig.FreeFalling:Fire(true)
			elseif last == State.Freefall and sig.FreeFalling then
				sig.FreeFalling:Fire(false)
			end
			last = now
		end
		local root = root_part(h.Parent)
		local speed = 0
		if root then
			local v = velocity(root)
			speed = Vector3.new(v.X, 0, v.Z).Magnitude
		end
		if sig.Running and math.abs(speed - last_speed) > 0.1 then
			last_speed = speed
			sig.Running:Fire(speed)
		end
	end)
end

------------------------------------------------------------------------------
-- What each kind of object gets. Groups are class names and families
-- (BasePart, GuiObject, GuiButton, ...); "Instance" is everything.
------------------------------------------------------------------------------

local props = {} -- group -> name -> default
local getters = {} -- group -> name -> function(self, slot)
local setters = {} -- group -> name -> function(self, value, slot)
local methods = {} -- group -> name -> function(self, ...)
local signals = {} -- group -> name -> true or function(self, signal, slot) that links it up

local function add(t, group, entries)
	t[group] = t[group] or {}
	for k, v in pairs(entries) do
		t[group][k] = v
	end
end

local function plain_signals(group, list)
	local t = {}
	for _, n in ipairs(list) do
		t[n] = true
	end
	add(signals, group, t)
end

local function stored(s, key, default)
	local v = s.values[key]
	if v == nil then
		return default
	end
	return v
end

-- Instance -------------------------------------------------------------------

add(props, "Instance", { Archivable = true })
add(methods, "Instance", {
	GetDebugId = function(self)
		return tostring(self)
	end,
	IsPropertyModified = function()
		return false
	end,
	ResetPropertyToDefault = function() end,
	GetStyled = function()
		return nil
	end,
})

-- Parts ----------------------------------------------------------------------

local surfaces = {}
for _, face in ipairs({ "Top", "Bottom", "Front", "Back", "Left", "Right" }) do
	surfaces[face .. "Surface"] = "SurfaceType.Smooth"
	surfaces[face .. "SurfaceInput"] = "InputType.NoInput"
	surfaces[face .. "ParamA"] = -0.5
	surfaces[face .. "ParamB"] = 0.5
end
add(props, "BasePart", surfaces)
add(props, "BasePart", {
	PivotOffset = CFrame.new(),
	Locked = false,
	Massless = false,
	RootPriority = 0,
	CustomPhysicalProperties = NIL,
	CollisionGroupId = 0,
	Elasticity = 0.5,
	Friction = 0.3,
	ReceiveAge = 0,
	ResizeIncrement = 1,
	TextureID = "",
	DoubleSided = false,
	EnableFluidForces = true,
	AudioCanCollide = true,
	CollisionFidelity = "CollisionFidelity.Default",
	RenderFidelity = "RenderFidelity.Automatic",
	FormFactor = NIL,
})
add(getters, "BasePart", {
	Velocity = function(self)
		return velocity(self)
	end,
	RotVelocity = function(self)
		return self.AssemblyAngularVelocity
	end,
	Rotation = function(self)
		return self.Orientation
	end,
	Mass = function(self)
		return self:GetMass()
	end,
	AssemblyRootPart = function(self)
		return self
	end,
	AssemblyCenterOfMass = function(self)
		return self.Position
	end,
	CenterOfMass = function(self)
		return self.Position
	end,
	ExtentsSize = function(self)
		return self.Size
	end,
	MeshSize = function(self)
		return self.Size
	end,
})
add(setters, "BasePart", {
	Velocity = function(self, v)
		self.AssemblyLinearVelocity = v
	end,
	RotVelocity = function(self, v)
		self.AssemblyAngularVelocity = v
	end,
	Rotation = function(self, v)
		self.Orientation = v
	end,
})
add(methods, "BasePart", {
	CanCollideWith = function(self, other)
		return self.CanCollide and other.CanCollide
	end,
	Resize = function()
		return true
	end,
	GetJoints = function()
		return {}
	end,
	MakeJoints = function() end,
	BreakJoints = function(self)
		for _, child in ipairs(self:GetChildren()) do
			if child:IsA("JointInstance") or child:IsA("WeldConstraint") then
				child:Destroy()
			end
		end
	end,
	GetRootPart = function(self)
		return self
	end,
	IsGrounded = function(self)
		return self.Anchored
	end,
	GetNetworkOwner = function()
		return nil
	end,
	SetNetworkOwner = function() end,
	SetNetworkOwnershipAuto = function() end,
	GetNetworkOwnershipAuto = function()
		return true
	end,
	CanSetNetworkOwnership = function()
		return true, nil
	end,
	GetVelocityAtPosition = function(self)
		return velocity(self)
	end,
	GetRenderCFrame = function(self)
		return self.CFrame
	end,
	GetConnectedParts = function(self)
		return { self }
	end,
})

-- Models ---------------------------------------------------------------------

add(props, "Model", {
	LevelOfDetail = "ModelLevelOfDetail.Automatic",
	ModelStreamingMode = "ModelStreamingMode.Default",
})
add(getters, "Model", {
	WorldPivot = function(self, s)
		return stored(s, "WorldPivot", nil) or self:GetPivot()
	end,
})
add(setters, "Model", {
	WorldPivot = function(_, v, s)
		s.values.WorldPivot = v
	end,
})
add(methods, "Model", {
	GetScale = function()
		return stored(CURRENT, "__scale", 1)
	end,
	ScaleTo = function(self, scale)
		local s = CURRENT
		if typeof(scale) ~= "number" or scale <= 0 then
			error("ScaleTo expects a number above 0", 2)
		end
		local by = scale / stored(s, "__scale", 1)
		local pivot = self:GetPivot().Position
		for _, part in ipairs(self:GetDescendants()) do
			if part:IsA("BasePart") then
				local cf = part.CFrame
				local turn = CFrame.new(cf.Position):Inverse() * cf
				part.Size *= by
				part.CFrame = CFrame.new(pivot + (cf.Position - pivot) * by) * turn
			end
		end
		s.values.__scale = scale
	end,
	GetModelCFrame = function(self)
		return self:GetPivot()
	end,
	GetPrimaryPartCFrame = function(self)
		return self.PrimaryPart and self.PrimaryPart.CFrame or self:GetPivot()
	end,
	BreakJoints = function(self)
		for _, d in ipairs(self:GetDescendants()) do
			if d:IsA("JointInstance") or d:IsA("WeldConstraint") then
				d:Destroy()
			end
		end
	end,
	MakeJoints = function() end,
	ResetOrientationToIdentity = function() end,
	AddPersistentPlayer = function() end,
	RemovePersistentPlayer = function() end,
})

-- Workspace ------------------------------------------------------------------

add(props, "Workspace", {
	StreamingEnabled = false,
	FilteringEnabled = true,
	AllowThirdPartySales = false,
	GlobalWind = Vector3.zero,
	AirDensity = 0.0012,
	TouchesUseCollisionGroups = false,
	SignalBehavior = NIL,
})
add(methods, "Workspace", {
	Blockcast = function(_, cf, size, direction, params)
		return sweep(cf, size, direction, params)
	end,
	Spherecast = function(_, position, radius, direction, params)
		return sweep(CFrame.new(position), Vector3.one * (radius * 2), direction, params)
	end,
	Shapecast = function(_, part, direction, params)
		return sweep(part.CFrame, part.Size, direction, params)
	end,
	GetServerTimeNow = function()
		return tick()
	end,
	GetRealPhysicsFPS = function()
		return 60
	end,
	GetNumAwakeParts = function(self)
		local n = 0
		for _, d in ipairs(self:GetDescendants()) do
			if d:IsA("BasePart") and not d.Anchored then
				n += 1
			end
		end
		return n
	end,
	PGSIsEnabled = function()
		return true
	end,
	BulkMoveTo = function(_, parts, cframes)
		for i, part in ipairs(parts) do
			part.CFrame = cframes[i]
		end
	end,
	FindPartsInRegion3 = function(_, region, ignore, max)
		local params = OverlapParams.new()
		if ignore then
			params.FilterDescendantsInstances = { ignore }
		end
		local found = workspace:GetPartBoundsInBox(region.CFrame, region.Size, params)
		if max and #found > max then
			found = table.move(found, 1, max, 1, {})
		end
		return found
	end,
	FindPartsInRegion3WithIgnoreList = function(_, region, ignore, max)
		local params = OverlapParams.new()
		params.FilterDescendantsInstances = ignore or {}
		local found = workspace:GetPartBoundsInBox(region.CFrame, region.Size, params)
		if max and #found > max then
			found = table.move(found, 1, max, 1, {})
		end
		return found
	end,
	FindPartsInRegion3WithWhiteList = function(_, region, list, max)
		local params = OverlapParams.new()
		params.FilterType = Enum.RaycastFilterType.Include
		params.FilterDescendantsInstances = list or {}
		local found = workspace:GetPartBoundsInBox(region.CFrame, region.Size, params)
		if max and #found > max then
			found = table.move(found, 1, max, 1, {})
		end
		return found
	end,
	IsRegion3Empty = function(_, region, ignore)
		return #workspace:FindPartsInRegion3(region, ignore, 1) == 0
	end,
})

-- Humanoids ------------------------------------------------------------------

add(props, "Humanoid", {
	HipHeight = 2,
	AutoRotate = true,
	UseJumpPower = true,
	JumpHeight = 7.2,
	DisplayDistanceType = "HumanoidDisplayDistanceType.Viewer",
	HealthDisplayType = "HumanoidHealthDisplayType.DisplayWhenDamaged",
	NameDisplayDistance = 100,
	HealthDisplayDistance = 100,
	RequiresNeck = true,
	BreakJointsOnDeath = true,
	PlatformStand = false,
	MaxSlopeAngle = 89,
	EvaluateStateMachine = true,
	RigType = "HumanoidRigType.R15",
	AutoJumpEnabled = true,
	CameraOffset = Vector3.zero,
	TargetPoint = Vector3.zero,
	WalkToPart = NIL,
	CollisionType = "HumanoidCollisionType.InnerBox",
})
add(getters, "Humanoid", {
	RootPart = function(self)
		return root_part(self.Parent)
	end,
	FloorMaterial = function(self, s)
		local state = humanoid_state(self, s)
		if state == State.Jumping or state == State.Freefall then
			return Enum.Material.Air or Enum.Material.Plastic
		end
		local root = root_part(self.Parent)
		if root then
			local params = RaycastParams.new()
			params.FilterDescendantsInstances = { self.Parent }
			local hit = workspace:Raycast(root.Position, Vector3.new(0, -6, 0), params)
			if hit then
				return hit.Material
			end
		end
		return Enum.Material.Air or Enum.Material.Plastic
	end,
	WalkToPoint = function(self, s)
		return stored(s, "WalkToPoint", Vector3.zero)
	end,
})
add(setters, "Humanoid", {
	WalkToPoint = function(self, v, s)
		s.values.WalkToPoint = v
		self:MoveTo(v)
	end,
})
add(methods, "Humanoid", {
	GetState = function(self)
		return humanoid_state(self, CURRENT)
	end,
	ChangeState = function(self, state)
		local s = CURRENT
		local old = humanoid_state(self, s)
		if state == State.Jumping then
			self.Jump = true
		elseif state == State.Dead then
			self.Health = 0
		end
		s.values.__forced = state
		s.values.__forced_until = time() + 0.25
		if s.signals.StateChanged and old ~= state then
			s.signals.StateChanged:Fire(old, state)
		end
	end,
	SetStateEnabled = function(_, state, enabled)
		local s = CURRENT
		s.values.__enabled = s.values.__enabled or {}
		s.values.__enabled[state] = enabled
		if s.signals.StateEnabledChanged then
			s.signals.StateEnabledChanged:Fire(state, enabled)
		end
	end,
	GetStateEnabled = function(_, state)
		local e = CURRENT.values.__enabled
		return not e or e[state] ~= false
	end,
	Move = function(self, direction, relative)
		local root = root_part(self.Parent)
		if not root then
			return
		end
		if relative and workspace.CurrentCamera then
			direction = workspace.CurrentCamera.CFrame:VectorToWorldSpace(direction)
		end
		local flat = Vector3.new(direction.X, 0, direction.Z)
		if flat.Magnitude < 1e-3 then
			self:MoveTo(root.Position)
		else
			self:MoveTo(root.Position + flat.Unit * 8)
		end
	end,
	GetMoveVelocity = function(self)
		local root = root_part(self.Parent)
		local v = root and velocity(root) or Vector3.zero
		return Vector3.new(v.X, 0, v.Z)
	end,
	GetAccessories = function(self)
		local out = {}
		if self.Parent then
			for _, child in ipairs(self.Parent:GetChildren()) do
				if child:IsA("Accessory") then
					table.insert(out, child)
				end
			end
		end
		return out
	end,
	AddAccessory = function(self, accessory)
		accessory.Parent = self.Parent
	end,
	RemoveAccessories = function(self)
		for _, a in ipairs(self:GetAccessories()) do
			a:Destroy()
		end
	end,
	GetLimb = function()
		return Enum.Limb.Unknown
	end,
	ApplyDescription = function() end,
	ApplyDescriptionReset = function() end,
	BuildRigFromAttachments = function() end,
	GetBodyPartR15 = function()
		return nil
	end,
	ReplaceBodyPartR15 = function()
		return false
	end,
})
local function state_signal(self, _, s)
	watch_humanoid(self, s)
end
add(signals, "Humanoid", {
	StateChanged = state_signal,
	Running = state_signal,
	Jumping = state_signal,
	FreeFalling = state_signal,
	Climbing = true,
	GettingUp = true,
	Swimming = true,
	Strafing = true,
	PlatformStanding = true,
	FallingDown = true,
	Ragdoll = true,
	StateEnabledChanged = true,
	AnimationPlayed = true,
	Touched = true,
	ApplyDescriptionFinished = true,
})

-- GUI ------------------------------------------------------------------------

add(props, "GuiObject", {
	Rotation = 0,
	BorderColor3 = Color3.fromRGB(27, 42, 53),
	BorderMode = "BorderMode.Outline",
	ClipsDescendants = false,
	Active = false,
	Selectable = false,
	SizeConstraint = "SizeConstraint.RelativeXY",
	AutomaticSize = "AutomaticSize.None",
	Interactable = true,
	SelectionImageObject = NIL,
	SelectionOrder = 0,
	NextSelectionUp = NIL,
	NextSelectionDown = NIL,
	NextSelectionLeft = NIL,
	NextSelectionRight = NIL,
	Style = NIL,
	AutoLocalize = true,
	RootLocalizationTable = NIL,
	Font = "Font.SourceSans",
	RichText = false,
	TextStrokeTransparency = 1,
	TextStrokeColor3 = Color3.new(0, 0, 0),
	TextTruncate = "TextTruncate.None",
	LineHeight = 1,
	MaxVisibleGraphemes = -1,
	TextDirection = "TextDirection.Auto",
	AutoButtonColor = true,
	Modal = false,
	Selected = false,
	CursorPosition = -1,
	SelectionStart = -1,
	MultiLine = false,
	TextEditable = true,
	ShowNativeInput = true,
	ResampleMode = "ResamplerMode.Default",
	SliceCenter = NIL,
	SliceScale = 1,
	TileSize = NIL,
	HoverImage = "",
	PressedImage = "",
	ScrollBarImageColor3 = Color3.new(1, 1, 1),
	ScrollBarImageTransparency = 0,
	ScrollingDirection = "ScrollingDirection.XY",
	ElasticBehavior = "ElasticBehavior.WhenScrollable",
	VerticalScrollBarInset = "ScrollBarInset.None",
	HorizontalScrollBarInset = "ScrollBarInset.None",
	AutomaticCanvasSize = "AutomaticSize.None",
	TopImage = "",
	MidImage = "",
	BottomImage = "",
	ScrollingEnabled = true,
})
add(getters, "GuiObject", {
	AbsoluteSize = function(self)
		return abs_size(self)
	end,
	AbsolutePosition = function(self)
		return abs_position(self)
	end,
	AbsoluteRotation = function(self, s)
		return stored(s, "Rotation", 0)
	end,
	AbsoluteWindowSize = function(self)
		return abs_size(self)
	end,
	FontFace = function(self, s)
		return stored(s, "FontFace", nil) or Font.fromEnum(stored(s, "Font", Enum.Font.SourceSans))
	end,
	ContentText = function(self)
		return self.Text
	end,
	TextFits = function()
		return true
	end,
	TextBounds = function(self)
		local ok, text = pcall(function()
			return self.Text
		end)
		local size = self.TextSize or 14
		return Vector2.new(#(ok and text or "") * size * 0.5, size)
	end,
	IsLoaded = function()
		return true
	end,
	GuiState = function()
		return NIL
	end,
})
add(methods, "GuiObject", {
	TweenPosition = function(self, position, direction, style, time, override, callback)
		return tween_gui(self, { Position = position }, direction, style, time, override, callback)
	end,
	TweenSize = function(self, size, direction, style, time, override, callback)
		return tween_gui(self, { Size = size }, direction, style, time, override, callback)
	end,
	TweenSizeAndPosition = function(self, size, position, direction, style, time, override, callback)
		return tween_gui(self, { Size = size, Position = position }, direction, style, time, override, callback)
	end,
	CaptureFocus = function() end,
	ReleaseFocus = function() end,
	IsFocused = function()
		return false
	end,
	ScrollToTop = function() end,
})
plain_signals("GuiObject", {
	"InputBegan", "InputEnded", "InputChanged", "MouseMoved", "MouseWheelForward", "MouseWheelBackward",
	"TouchTap", "TouchLongPress", "TouchPan", "TouchPinch", "TouchRotate", "TouchSwipe", "SelectionGained",
	"SelectionLost", "SelectionChanged", "ReturnPressedFromOnScreenKeyboard",
})
-- A press on a button: libroxel knows about whole clicks, so the down and
-- up halves arrive together, just before the click.
local function click_half(self, sig)
	self.MouseButton1Click:Connect(function()
		local mouse = game:GetService("Players").LocalPlayer
		local x, y = 0, 0
		if mouse then
			local m = mouse:GetMouse()
			x, y = m.X, m.Y
		end
		sig:Fire(x, y)
	end)
end
add(signals, "GuiButton", {
	MouseButton1Down = click_half,
	MouseButton1Up = click_half,
	MouseButton2Click = true,
	MouseButton2Down = true,
	MouseButton2Up = true,
})

add(props, "LayerCollector", {
	IgnoreGuiInset = false,
	DisplayOrder = 0,
	ZIndexBehavior = "ZIndexBehavior.Sibling",
	ScreenInsets = "ScreenInsets.CoreUISafeInsets",
	ClipToDeviceSafeArea = true,
	SafeAreaCompatibility = NIL,
	OnTopOfCoreBlur = false,
	SizingMode = "SurfaceGuiSizingMode.FixedSize",
	PixelsPerStud = 50,
	CanvasSize = Vector2.new(800, 600),
	Face = "NormalId.Front",
	LightInfluence = 1,
	Brightness = 1,
	Adornee = NIL,
	MaxDistance = 0,
})
add(getters, "LayerCollector", {
	AbsoluteSize = function()
		return viewport()
	end,
	AbsolutePosition = function()
		return Vector2.zero
	end,
})

local layout = {
	SortOrder = "SortOrder.LayoutOrder",
	HorizontalAlignment = "HorizontalAlignment.Left",
	VerticalAlignment = "VerticalAlignment.Top",
	Wraps = false,
	StartCorner = "StartCorner.TopLeft",
	FillDirectionMaxCells = 0,
}
local function content_size(self)
	local parent = self.Parent
	if not parent then
		return Vector2.zero
	end
	local w, h = 0, 0
	local vertical = tostring(self.FillDirection) ~= "Enum.FillDirection.Horizontal"
	local pad = 0
	pcall(function()
		local p = abs_size(parent)
		pad = self.Padding.Scale * (vertical and p.Y or p.X) + self.Padding.Offset
	end)
	local n = 0
	for _, child in ipairs(parent:GetChildren()) do
		if child:IsA("GuiObject") and child.Visible then
			local size = abs_size(child)
			n += 1
			if vertical then
				w, h = math.max(w, size.X), h + size.Y
			else
				w, h = w + size.X, math.max(h, size.Y)
			end
		end
	end
	local gaps = math.max(n - 1, 0) * pad
	return vertical and Vector2.new(w, h + gaps) or Vector2.new(w + gaps, h)
end
add(props, "UIListLayout", layout)
add(props, "UIGridLayout", layout)
add(getters, "UIListLayout", { AbsoluteContentSize = content_size })
add(getters, "UIGridLayout", {
	AbsoluteContentSize = content_size,
	AbsoluteCellSize = function(self)
		local cell = self.CellSize
		return Vector2.new(cell.X.Offset, cell.Y.Offset)
	end,
	AbsoluteCellCount = function(self)
		return Vector2.new(1, #self.Parent:GetChildren())
	end,
})
add(methods, "UIListLayout", { ApplyLayout = function() end })
add(methods, "UIGridLayout", { ApplyLayout = function() end })

-- Things in the world --------------------------------------------------------

add(props, "ProximityPrompt", {
	RequiresLineOfSight = true,
	Style = "ProximityPromptStyle.Default",
	Exclusivity = "ProximityPromptExclusivity.OnePerButton",
	ClickablePrompt = true,
	AutoLocalize = true,
	UIOffset = Vector2.zero,
	GamepadKeyCode = NIL,
	RootLocalizationTable = NIL,
})
add(methods, "ProximityPrompt", {
	InputHoldBegin = function() end,
	InputHoldEnd = function() end,
})
plain_signals("ProximityPrompt", { "PromptShown", "PromptHidden", "PromptButtonHoldBegan", "PromptButtonHoldEnded" })
add(props, "ClickDetector", { CursorIcon = "" })
plain_signals("ClickDetector", { "MouseHoverEnter", "MouseHoverLeave", "RightMouseClick" })
add(props, "Tool", {
	RequiresHandle = true,
	ToolTip = "",
	CanBeDropped = true,
	ManualActivationOnly = false,
	Grip = CFrame.new(),
	GripPos = Vector3.zero,
	GripForward = Vector3.new(0, 0, -1),
	GripRight = Vector3.new(1, 0, 0),
	GripUp = Vector3.new(0, 1, 0),
	TextureId = "",
})
add(props, "SpawnLocation", { Duration = 10, AllowTeamChangeOnTouch = false })
add(props, "Light", { Shadows = false })
add(props, "SurfaceLight", {
	Brightness = 1,
	Color = Color3.new(1, 1, 1),
	Range = 16,
	Angle = 90,
	Face = "NormalId.Front",
	Enabled = true,
})
add(props, "Sound", {
	IsLoaded = true,
	TimePosition = 0,
	RollOffMaxDistance = 10000,
	RollOffMinDistance = 10,
	RollOffMode = "RollOffMode.Inverse",
	SoundGroup = NIL,
	PlayOnRemove = false,
	PlaybackRegionsEnabled = false,
})
add(getters, "Sound", {
	TimeLength = function(_, s)
		return stored(s, "TimeLength", 0)
	end,
	IsPlaying = function(self)
		return self.Playing
	end,
	IsPaused = function(self)
		return not self.Playing
	end,
	PlaybackLoudness = function(self)
		return self.Playing and 100 or 0
	end,
})
plain_signals("Sound", { "Loaded", "Played", "Paused", "Resumed", "Stopped", "DidLoop" })
add(props, "Lighting", {
	OutdoorAmbient = Color3.fromRGB(128, 128, 128),
	GlobalShadows = true,
	ShadowSoftness = 0.2,
	EnvironmentDiffuseScale = 0,
	EnvironmentSpecularScale = 0,
	ExposureCompensation = 0,
	GeographicLatitude = 0,
	ColorShift_Top = Color3.new(0, 0, 0),
	ColorShift_Bottom = Color3.new(0, 0, 0),
	Technology = "Technology.ShadowMap",
	LightingStyle = NIL,
	PrioritizeLightingQuality = true,
})
add(methods, "Lighting", {
	GetSunDirection = function(self)
		local hours = (self.ClockTime or 14) - 6
		local a = hours / 12 * math.pi
		return Vector3.new(math.cos(a), math.sin(a), 0.2).Unit
	end,
	GetMoonDirection = function(self)
		return -self:GetSunDirection()
	end,
	GetMoonPhase = function()
		return 0.75
	end,
})
plain_signals("Lighting", { "LightingChanged" })
add(props, "Camera", {
	Focus = CFrame.new(),
	NearPlaneZ = -0.1,
	HeadLocked = true,
	HeadScale = 1,
	VRTiltAndRollEnabled = false,
})
add(getters, "Camera", {
	DiagonalFieldOfView = function(self)
		return self.FieldOfView * 1.6
	end,
	MaxAxisFieldOfView = function(self)
		local size = self.ViewportSize
		return self.FieldOfView * math.max(1, size.X / math.max(size.Y, 1))
	end,
})
add(methods, "Camera", {
	ViewportPointToRay = function(self, x, y, depth)
		return screen_ray(self, x, y, depth)
	end,
	ScreenPointToRay = function(self, x, y, depth)
		return screen_ray(self, x, y, depth)
	end,
	GetRenderCFrame = function(self)
		return self.CFrame
	end,
	GetRoll = function()
		return 0
	end,
	SetRoll = function() end,
	Interpolate = function(self, cf, focus, duration)
		local info = TweenInfo.new(duration or 1)
		service("TweenService"):Create(self, info, { CFrame = cf }):Play()
		self.Focus = focus
	end,
	GetPartsObscuringTarget = function()
		return {}
	end,
	ZoomToExtents = function() end,
	PanUnits = function() end,
	TiltUnits = function()
		return true
	end,
	Zoom = function()
		return true
	end,
	GetLargestCutoffDistance = function()
		return 0
	end,
})
plain_signals("Camera", { "InterpolationFinished" })

-- Joints: the turn a motor's animation adds.
add(props, "Motor6D", { Transform = CFrame.new(), CurrentAngle = 0 })
add(props, "JointInstance", { Active = true })

-- Kept-for-scripts classes ---------------------------------------------------

add(props, "Sky", {
	SkyboxBk = "", SkyboxDn = "", SkyboxFt = "", SkyboxLf = "", SkyboxRt = "", SkyboxUp = "",
	SunTextureId = "", MoonTextureId = "", SkyboxOrientation = Vector3.zero,
})
-- (Atmosphere's Density, Haze and Color are real; these aren't drawn yet.)
add(props, "Atmosphere", { Offset = 0, Decay = Color3.fromRGB(106, 112, 125), Glare = 0 })
add(props, "PostEffect", { Enabled = true })
add(props, "BlurEffect", { Size = 24 })
add(props, "SunRaysEffect", { Intensity = 0.25, Spread = 1 })
add(props, "DepthOfFieldEffect", { FarIntensity = 0.75, FocusDistance = 0.05, InFocusRadius = 10, NearIntensity = 0.75 })
local strip = {
	Attachment0 = NIL, Attachment1 = NIL, Color = NIL, Enabled = true, LightEmission = 0, LightInfluence = 1,
	Texture = "", TextureLength = 1, TextureMode = "TextureMode.Stretch", TextureSpeed = 1, Transparency = NIL,
	FaceCamera = false, Width0 = 1, Width1 = 1, CurveSize0 = 0, CurveSize1 = 0, Segments = 10, ZOffset = 0,
	Lifetime = 2, MinLength = 0.1, MaxLength = 0, WidthScale = NIL, Brightness = 1,
}
add(props, "Beam", strip)
add(props, "Trail", strip)
add(methods, "Trail", { Clear = function() end })
add(methods, "Beam", { SetTextureOffset = function() end })
add(props, "SelectionBox", { SurfaceColor3 = Color3.fromRGB(13, 105, 172), SurfaceTransparency = 1, Transparency = 0 })
add(props, "Constraint", {
	Attachment0 = NIL, Attachment1 = NIL, Enabled = true, Visible = false, Color = NIL,
})
add(props, "AlignOrientation", {
	MaxAngularVelocity = math.huge, Mode = NIL, PrimaryAxisOnly = false, AlignType = "AlignType.Parallel",
})
add(props, "Torque", { RelativeTo = "ActuatorRelativeTo.Attachment0" })
add(props, "RodConstraint", { LimitsEnabled = false })
add(getters, "RodConstraint", {
	CurrentDistance = function(self)
		local a, b = self.Attachment0, self.Attachment1
		if a and b then
			return (a.WorldPosition - b.WorldPosition).Magnitude
		end
		return 0
	end,
})
add(props, "CylindricalConstraint", {
	LimitsEnabled = false, LowerLimit = 0, UpperLimit = 5, AngularActuatorType = NIL, InclinationAngle = 0,
})
add(props, "SpecialMesh", {
	MeshType = "MeshType.Head", MeshId = "", TextureId = "", Offset = Vector3.zero, Scale = Vector3.one,
	VertexColor = Vector3.one,
})
add(props, "Shirt", { ShirtTemplate = "", Color3 = Color3.new(1, 1, 1) })
add(props, "Pants", { PantsTemplate = "", Color3 = Color3.new(1, 1, 1) })
add(props, "CFrameValue", { Value = CFrame.new() })
add(props, "BrickColorValue", { Value = BrickColor.new("Medium stone grey") })
plain_signals("CFrameValue", { "Changed" })
plain_signals("BrickColorValue", { "Changed" })

-- Players and the game -------------------------------------------------------

add(props, "Players", {
	MaxPlayers = 50,
	PreferredPlayers = 50,
	BubbleChat = true,
	ClassicChat = false,
	UseStrafingAnimations = false,
	BanningEnabled = true,
})
local function player_named(name)
	for _, p in ipairs(game:GetService("Players"):GetPlayers()) do
		if p.Name == name then
			return p
		end
	end
end
local function player_with_id(id)
	return game:GetService("Players"):GetPlayerByUserId(id)
end
local function empty_pages()
	local pages = { IsFinished = true }
	function pages.GetCurrentPage()
		return {}
	end
	function pages.AdvanceToNextPageAsync() end
	return pages
end
add(methods, "Players", {
	GetUserIdFromNameAsync = function(_, name)
		local p = player_named(name)
		if not p then
			error("no one called " .. tostring(name) .. " is in this game", 2)
		end
		return p.UserId
	end,
	GetNameFromUserIdAsync = function(_, id)
		local p = player_with_id(id)
		if not p then
			error("no one with user id " .. tostring(id) .. " is in this game", 2)
		end
		return p.Name
	end,
	GetUserThumbnailAsync = function()
		return "", true
	end,
	GetFriendsAsync = function()
		return empty_pages()
	end,
	GetCharacterAppearanceInfoAsync = function()
		return {}
	end,
	GetCharacterAppearanceAsync = function()
		return Instance.new("Model")
	end,
	GetHumanoidDescriptionFromUserId = function()
		return nil
	end,
	GetHumanoidDescriptionFromOutfitId = function()
		return nil
	end,
	CreateHumanoidModelFromUserId = function()
		return Instance.new("Model")
	end,
	CreateHumanoidModelFromDescription = function()
		return Instance.new("Model")
	end,
	BanAsync = function(_, config)
		for _, id in ipairs(config and config.UserIds or {}) do
			local p = player_with_id(id)
			if p then
				p:Kick(config.DisplayReason or "")
			end
		end
	end,
	UnbanAsync = function() end,
	GetBanHistoryAsync = function()
		return empty_pages()
	end,
})
plain_signals("Players", { "PlayerMembershipChanged", "UserSubscriptionStatusChanged" })
add(props, "Player", {
	AccountAge = 0,
	MembershipType = "MembershipType.None",
	FollowUserId = 0,
	CameraMaxZoomDistance = 400,
	CameraMinZoomDistance = 0.5,
	CameraMode = "CameraMode.Classic",
	DevComputerMovementMode = "DevComputerMovementMode.UserChoice",
	DevTouchMovementMode = "DevTouchMovementMode.UserChoice",
	DevCameraOcclusionMode = "DevCameraOcclusionMode.Zoom",
	DevEnableMouseLock = true,
	HealthDisplayDistance = 100,
	NameDisplayDistance = 100,
	AutoJumpEnabled = true,
	LocaleId = "en-us",
	ReplicationFocus = NIL,
	RespawnLocation = NIL,
	GameplayPaused = false,
	CanLoadCharacterAppearance = true,
	CharacterAppearanceId = 0,
	HasVerifiedBadge = false,
	PartyId = "",
})
add(methods, "Player", {
	IsFriendsWith = function()
		return false
	end,
	GetRankInGroup = function()
		return 0
	end,
	GetRoleInGroup = function()
		return "Guest"
	end,
	IsInGroup = function()
		return false
	end,
	GetFriendsOnline = function()
		return {}
	end,
	HasAppearanceLoaded = function()
		return true
	end,
	ClearCharacterAppearance = function() end,
	RequestStreamAroundAsync = function() end,
	GetJoinData = function()
		return {}
	end,
	GetFriendStatus = function()
		return Enum.FriendStatus.NotFriend
	end,
	SetSuperSafeChat = function() end,
	GetNetworkPing = function()
		return 0
	end,
	DistanceFromCharacter = function(self, point)
		local root = root_part(self.Character)
		return root and (root.Position - point).Magnitude or 0
	end,
	IsVerified = function()
		return false
	end,
})
plain_signals("Player", { "Chatted", "Idled", "CharacterAppearanceLoaded", "OnTeleport" })
add(props, "StarterPlayer", {
	CameraMaxZoomDistance = 400,
	CameraMinZoomDistance = 0.5,
	CameraMode = "CameraMode.Classic",
	DevComputerMovementMode = "DevComputerMovementMode.UserChoice",
	DevTouchMovementMode = "DevTouchMovementMode.UserChoice",
	DevCameraOcclusionMode = "DevCameraOcclusionMode.Zoom",
	EnableMouseLockOption = true,
	AutoJumpEnabled = true,
	LoadCharacterAppearance = true,
	UserEmotesEnabled = true,
	HealthDisplayDistance = 100,
	NameDisplayDistance = 100,
	CharacterUseJumpPower = true,
	CharacterJumpHeight = 7.2,
	CharacterMaxSlopeAngle = 89,
	AllowCustomAnimations = true,
	CharacterBreakJointsOnDeath = true,
})
add(methods, "StarterGui", {
	SetCore = function(_, name, value)
		CURRENT.values["core:" .. tostring(name)] = value
	end,
	GetCore = function(_, name)
		return CURRENT.values["core:" .. tostring(name)]
	end,
	SetCoreGuiEnabled = function(_, kind, enabled)
		CURRENT.values["coregui:" .. tostring(kind)] = enabled
	end,
	GetCoreGuiEnabled = function(_, kind)
		local s = CURRENT
		local all = s.values["coregui:" .. tostring(Enum.CoreGuiType.All)]
		local one = s.values["coregui:" .. tostring(kind)]
		if one ~= nil then
			return one
		end
		return all ~= false
	end,
	RegisterSetCore = function() end,
	RegisterGetCore = function() end,
	GetGuiObjectsAtPosition = function()
		return {}
	end,
})
add(props, "StarterGui", { ShowDevelopmentGui = true, ScreenOrientation = NIL, RtlTextSupport = NIL })
add(props, "DataModel", {
	PlaceVersion = 1,
	CreatorId = 0,
	CreatorType = NIL,
	GameId = 0,
	PrivateServerId = "",
	PrivateServerOwnerId = 0,
	Genre = NIL,
})
add(methods, "DataModel", {
	FindService = function(self, name)
		local ok, found = pcall(self.GetService, self, name)
		return ok and found or nil
	end,
	GetJobsInfo = function()
		return {}
	end,
	SetPlaceId = function() end,
	SetUniverseId = function() end,
})
plain_signals("DataModel", { "Loaded", "GraphicsQualityChangeRequest" })

------------------------------------------------------------------------------
-- Services that are objects of their own rather than instances
------------------------------------------------------------------------------

local render_steps = {}
add(methods, "RunService", {
	BindToRenderStep = function(self, name, _, fn)
		if render_steps[name] then
			render_steps[name]:Disconnect()
		end
		render_steps[name] = self.RenderStepped:Connect(fn)
	end,
	UnbindFromRenderStep = function(_, name)
		if render_steps[name] then
			render_steps[name]:Disconnect()
			render_steps[name] = nil
		end
	end,
	IsRunMode = function()
		return true
	end,
	IsEdit = function()
		return false
	end,
	Run = function() end,
	Pause = function() end,
	Stop = function() end,
})
local function alias(name)
	return function(self, sig)
		self[name]:Connect(function(...)
			sig:Fire(...)
		end)
	end
end
add(signals, "RunService", {
	PreSimulation = alias("Stepped"),
	PostSimulation = alias("Heartbeat"),
	PreRender = alias("RenderStepped"),
	PreAnimation = alias("Stepped"),
})

local function mouse()
	local p = game:GetService("Players").LocalPlayer
	return p and p:GetMouse()
end
add(props, "UserInputService", {
	MouseIconEnabled = true,
	MouseIcon = "",
	MouseDeltaSensitivity = 1,
	TouchEnabled = false,
	KeyboardEnabled = true,
	MouseEnabled = true,
	GamepadEnabled = false,
	VREnabled = false,
	AccelerometerEnabled = false,
	GyroscopeEnabled = false,
	ModalEnabled = false,
	OnScreenKeyboardVisible = false,
	OnScreenKeyboardPosition = Vector2.zero,
	OnScreenKeyboardSize = Vector2.zero,
	PreferredInput = NIL,
})
add(getters, "UserInputService", {
	MouseBehavior = function(_, s)
		return stored(s, "MouseBehavior", Enum.MouseBehavior.Default)
	end,
})
add(setters, "UserInputService", {
	MouseBehavior = function(_, v, s)
		s.values.MouseBehavior = v
	end,
})
add(methods, "UserInputService", {
	GetMouseLocation = function()
		local m = mouse()
		return m and Vector2.new(m.X, m.Y) or Vector2.zero
	end,
	GetLastInputType = function()
		return Enum.UserInputType.Keyboard
	end,
	GetMouseDelta = function()
		return Vector2.zero
	end,
	GetKeysPressed = function()
		return {}
	end,
	GetMouseButtonsPressed = function()
		return {}
	end,
	GetFocusedTextBox = function()
		return nil
	end,
	GetConnectedGamepads = function()
		return {}
	end,
	GetNavigationGamepads = function()
		return {}
	end,
	GetGamepadConnected = function()
		return false
	end,
	GetGamepadState = function()
		return {}
	end,
	GetSupportedGamepadKeyCodes = function()
		return {}
	end,
	GamepadSupports = function()
		return false
	end,
	GetStringForKeyCode = function(_, key)
		return key.Name
	end,
	GetImageForKeyCode = function()
		return ""
	end,
	GetDeviceRotation = function()
		return nil, CFrame.new()
	end,
	GetDeviceGravity = function()
		return nil
	end,
	IsNavigationGamepad = function()
		return false
	end,
	SetNavigationGamepad = function() end,
	IsGamepadButtonDown = function()
		return false
	end,
})
add(signals, "UserInputService", {
	JumpRequest = function(self, sig)
		self.InputBegan:Connect(function(input, typing)
			if not typing and input.KeyCode == Enum.KeyCode.Space then
				sig:Fire()
			end
		end)
	end,
	InputChanged = true,
	TouchStarted = true,
	TouchEnded = true,
	TouchMoved = true,
	TouchTap = true,
	TouchTapInWorld = true,
	TouchLongPress = true,
	TouchPan = true,
	TouchPinch = true,
	TouchRotate = true,
	TouchSwipe = true,
	LastInputTypeChanged = true,
	GamepadConnected = true,
	GamepadDisconnected = true,
	TextBoxFocused = true,
	TextBoxFocusReleased = true,
	WindowFocused = true,
	WindowFocusReleased = true,
	PointerAction = true,
	DeviceAccelerationChanged = true,
	DeviceGravityChanged = true,
	DeviceRotationChanged = true,
})

-- Ordered data stores, kept in one value of an ordinary data store.
local function ordered_store(name, scope)
	local backing = service("DataStoreService"):GetDataStore(name .. " (ordered)", scope)
	local KEY = "values"
	local store = {}
	local function all()
		return backing:GetAsync(KEY) or {}
	end
	function store.GetAsync(_, key)
		return all()[key]
	end
	function store.SetAsync(_, key, value)
		if typeof(value) ~= "number" then
			error("ordered data stores only hold whole numbers", 2)
		end
		local t = all()
		t[key] = math.floor(value)
		backing:SetAsync(KEY, t)
	end
	function store.IncrementAsync(_, key, by)
		local t = all()
		t[key] = (t[key] or 0) + math.floor(by or 1)
		backing:SetAsync(KEY, t)
		return t[key]
	end
	function store.UpdateAsync(_, key, fn)
		local t = all()
		local new = fn(t[key])
		if new ~= nil then
			t[key] = new
			backing:SetAsync(KEY, t)
		end
		return t[key]
	end
	function store.RemoveAsync(_, key)
		local t = all()
		local old = t[key]
		t[key] = nil
		backing:SetAsync(KEY, t)
		return old
	end
	function store.GetSortedAsync(_, ascending, size, min, max)
		local rows = {}
		for key, value in pairs(all()) do
			if (not min or value >= min) and (not max or value <= max) then
				table.insert(rows, { key = key, value = value })
			end
		end
		table.sort(rows, function(a, b)
			if a.value == b.value then
				return a.key < b.key
			end
			if ascending then
				return a.value < b.value
			end
			return a.value > b.value
		end)
		size = math.max(1, size or 50)
		local page = 0
		local pages = { IsFinished = false }
		local function check()
			pages.IsFinished = (page + 1) * size >= #rows
		end
		function pages.GetCurrentPage()
			return table.move(rows, page * size + 1, math.min((page + 1) * size, #rows), 1, {})
		end
		function pages.AdvanceToNextPageAsync()
			page += 1
			check()
		end
		check()
		return pages
	end
	return store
end
add(methods, "DataStoreService", {
	GetOrderedDataStore = function(_, name, scope)
		return ordered_store(name, scope)
	end,
	GetRequestBudgetForRequestType = function()
		return 100
	end,
	ListDataStoresAsync = function()
		return empty_pages()
	end,
})
plain_signals("Path", { "Blocked", "Unblocked" })

------------------------------------------------------------------------------
-- Services libroxel only pretends to have
------------------------------------------------------------------------------

local function filter(text)
	local ok, result = pcall(function()
		return service("TextService"):FilterStringAsync(text, 0):GetNonChatStringForBroadcastAsync()
	end)
	return ok and result or text
end

local memory = { sorted = {}, queues = {}, hashes = {} }
local function memory_map(maps, name)
	local map = maps[name]
	if map then
		return map
	end
	local data = {}
	map = {}
	local function alive(key)
		local e = data[key]
		if e and e.expires and time() > e.expires then
			data[key] = nil
			return nil
		end
		return e
	end
	function map.GetAsync(_, key)
		local e = alive(key)
		if e then
			return e.value, e.sort
		end
	end
	function map.SetAsync(_, key, value, expiration, sort)
		data[key] = { value = value, sort = sort, expires = expiration and time() + expiration }
		return true
	end
	function map.UpdateAsync(_, key, fn, expiration)
		local e = alive(key)
		local value, sort = fn(e and e.value, e and e.sort)
		if value ~= nil then
			data[key] = { value = value, sort = sort, expires = expiration and time() + expiration }
		end
		return value
	end
	function map.RemoveAsync(_, key)
		data[key] = nil
	end
	function map.GetRangeAsync(_, direction, count, lower, upper)
		local rows = {}
		for key in pairs(data) do
			local e = alive(key)
			if e then
				table.insert(rows, { key = key, value = e.value, sortKey = e.sort })
			end
		end
		table.sort(rows, function(a, b)
			return a.key < b.key
		end)
		if direction == Enum.SortDirection.Descending then
			local r = {}
			for i = #rows, 1, -1 do
				table.insert(r, rows[i])
			end
			rows = r
		end
		local out = {}
		for _, row in ipairs(rows) do
			local k = row.key
			if (not lower or k > (lower.key or lower)) and (not upper or k < (upper.key or upper)) then
				table.insert(out, row)
				if #out >= (count or 200) then
					break
				end
			end
		end
		return out
	end
	function map.ListItemsAsync(self)
		local rows = self:GetRangeAsync(Enum.SortDirection.Ascending, 200)
		local pages = { IsFinished = true }
		function pages.GetCurrentPage()
			return rows
		end
		function pages.AdvanceToNextPageAsync() end
		return pages
	end
	maps[name] = map
	return map
end
local function memory_queue(name)
	local q = memory.queues[name]
	if q then
		return q
	end
	local items, next_id, taken = {}, 1, {}
	q = {}
	function q.AddAsync(_, value, _, priority)
		table.insert(items, { value = value, priority = priority or 0, order = next_id })
		next_id += 1
		table.sort(items, function(a, b)
			if a.priority == b.priority then
				return a.order < b.order
			end
			return a.priority > b.priority
		end)
	end
	function q.ReadAsync(_, count, all_or_nothing)
		count = count or 1
		if all_or_nothing and #items < count then
			return {}, ""
		end
		local out, read = {}, {}
		for _ = 1, math.min(count, #items) do
			local item = table.remove(items, 1)
			table.insert(out, item.value)
			table.insert(read, item)
		end
		local id = tostring(next_id)
		next_id += 1
		taken[id] = read
		return out, id
	end
	function q.RemoveAsync(_, id)
		taken[id] = nil
	end
	memory.queues[name] = q
	return q
end

local SERVICES = {
	TextChatService = {
		props = {
			ChatVersion = "ChatVersion.TextChatService",
			CreateDefaultTextChannels = true,
			CreateDefaultCommands = true,
			OnIncomingMessage = NIL,
		},
		signals = { "MessageReceived", "SendingMessage", "BubbleDisplayed" },
		methods = {
			CanUsersChatAsync = function()
				return true
			end,
			CanUserChatAsync = function()
				return true
			end,
			CanUsersDirectChatAsync = function()
				return true
			end,
			DisplayBubble = function() end,
		},
	},
	Chat = {
		props = { BubbleChatEnabled = true, LoadDefaultChat = true },
		signals = { "Chatted" },
		methods = {
			Chat = function() end,
			FilterStringAsync = function(_, text)
				return filter(text)
			end,
			FilterStringForBroadcast = function(_, text)
				return filter(text)
			end,
			CanUserChatAsync = function()
				return true
			end,
			CanUsersChatAsync = function()
				return true
			end,
			SetBubbleChatSettings = function() end,
			RegisterChatCallback = function() end,
			InvokeChatCallback = function() end,
		},
	},
	LocalizationService = {
		props = { SystemLocaleId = "en-us" },
		methods = (function()
			local function translator()
				return {
					LocaleId = "en-us",
					Translate = function(_, _, text)
						return text
					end,
					FormatByKey = function(_, key)
						return key
					end,
				}
			end
			return {
				GetTranslatorForPlayerAsync = translator,
				GetTranslatorForPlayer = translator,
				GetTranslatorForLocaleAsync = translator,
				GetCountryRegionForPlayerAsync = function()
					return "US"
				end,
				GetTableEntries = function()
					return {}
				end,
			}
		end)(),
	},
	GroupService = {
		methods = {
			GetGroupInfoAsync = function(_, id)
				return { Id = id, Name = "", Description = "", Roles = {}, Owner = nil }
			end,
			GetGroupsAsync = function()
				return {}
			end,
			GetAlliesAsync = function()
				return empty_pages()
			end,
			GetEnemiesAsync = function()
				return empty_pages()
			end,
		},
	},
	UserService = {
		methods = {
			GetUserInfosByUserIdsAsync = function(_, ids)
				local out = {}
				for _, id in ipairs(ids) do
					local p = player_with_id(id)
					if p then
						table.insert(out, { Id = id, Username = p.Name, DisplayName = p.DisplayName, HasVerifiedBadge = false })
					end
				end
				return out
			end,
		},
	},
	AssetService = {
		methods = {
			GetGamePlacesAsync = function()
				return empty_pages()
			end,
			GetBundleDetailsAsync = function()
				error("bundles aren't available in libroxel", 2)
			end,
			CreatePlaceAsync = function()
				error("making places from scripts isn't available in libroxel", 2)
			end,
		},
	},
	InsertService = {
		methods = {
			LoadAsset = function(_, id)
				error("loading asset " .. tostring(id) .. " isn't available in libroxel: put the model in the game instead", 2)
			end,
			LoadAssetVersion = function()
				error("loading assets isn't available in libroxel", 2)
			end,
			GetFreeModels = function()
				return {}
			end,
			GetFreeDecals = function()
				return {}
			end,
			CreateMeshPartAsync = function()
				return Instance.new("Part")
			end,
		},
	},
	MemoryStoreService = {
		methods = {
			GetSortedMap = function(_, name)
				return memory_map(memory.sorted, name)
			end,
			GetHashMap = function(_, name)
				return memory_map(memory.hashes, name)
			end,
			GetQueue = function(_, name)
				return memory_queue(name)
			end,
		},
	},
	SocialService = {
		signals = { "GameInvitePromptClosed", "CallInviteStateChanged" },
		methods = {
			CanSendGameInviteAsync = function()
				return false
			end,
			PromptGameInvite = function() end,
			CanSendCallInviteAsync = function()
				return false
			end,
		},
	},
	VoiceChatService = {
		methods = {
			IsVoiceEnabledForUserIdAsync = function()
				return false
			end,
		},
	},
	ProximityPromptService = {
		props = { Enabled = true, MaxPromptsVisible = 16, MaxIndicatorsVisible = 16 },
		signals = {
			"PromptTriggered", "PromptTriggerEnded", "PromptShown", "PromptHidden", "PromptButtonHoldBegan",
			"PromptButtonHoldEnded",
		},
	},
	KeyframeSequenceProvider = {
		methods = {
			RegisterKeyframeSequence = function()
				return ""
			end,
			GetKeyframeSequenceAsync = function()
				error("keyframe sequences aren't available in libroxel", 2)
			end,
		},
	},
	LogService = {
		signals = { "MessageOut" },
		methods = {
			GetLogHistory = function()
				return {}
			end,
		},
	},
	ScriptContext = { signals = { "Error" } },
	AnalyticsService = (function()
		local quiet = {}
		for _, n in ipairs({
			"LogCustomEvent", "LogEconomyEvent", "LogFunnelStepEvent", "LogOnboardingFunnelStepEvent",
			"LogProgressionEvent", "LogProgressionCompleteEvent", "LogProgressionFailEvent",
			"LogProgressionStartEvent", "FireCustomEvent", "FireEvent", "FireInGameEconomyEvent",
			"FireLogEvent", "FirePlayerProgressionEvent",
		}) do
			quiet[n] = function() end
		end
		return { methods = quiet }
	end)(),
	Stats = {
		props = { DataReceiveKbps = 0, DataSendKbps = 0, HeartbeatTimeMs = 16, PhysicsReceiveKbps = 0, PhysicsSendKbps = 0, InstanceCount = 0, PrimitivesCount = 0 },
		methods = {
			GetTotalMemoryUsageMb = function()
				return collectgarbage("count") / 1024
			end,
			GetMemoryUsageMbForTag = function()
				return 0
			end,
		},
	},
	TestService = {
		methods = {
			Check = function(_, ok, text)
				if not ok then
					warn(text)
				end
			end,
			Checkpoint = function() end,
			Done = function() end,
			Error = function(_, text)
				warn(text)
			end,
			Fail = function(_, text)
				warn(text)
			end,
			Message = function(_, text)
				print(text)
			end,
			Warn = function(_, ok, text)
				if not ok then
					warn(text)
				end
			end,
		},
	},
}

local function resolve(v)
	-- Enum defaults are written "Kind.Item" and looked up when first read.
	if type(v) == "string" and v:find("^%u%a+%.%w+$") then
		local kind, item = v:match("^(%a+)%.(%w+)$")
		local ok, e = pcall(function()
			return Enum[kind][item]
		end)
		if ok and e ~= nil then
			return e
		end
	end
	return v
end
local made = {}
function L.service(name)
	if made[name] then
		return made[name]
	end
	local def = SERVICES[name]
	if not def then
		return nil
	end
	local values, sigs = {}, {}
	for _, n in ipairs(def.signals or {}) do
		sigs[n] = new_signal()
	end
	local obj = { Name = name, ClassName = name }
	setmetatable(obj, {
		__index = function(_, k)
			if def.methods and def.methods[k] then
				return def.methods[k]
			end
			if sigs[k] then
				return sigs[k]
			end
			local v = values[k]
			if v ~= nil then
				return v
			end
			local p = def.props and def.props[k]
			if p ~= nil then
				if p == NIL then
					return nil
				end
				return resolve(p)
			end
			if k == "IsA" then
				return function(_, n)
					return n == name or n == "Instance"
				end
			end
			if k == "GetFullName" then
				return function()
					return name
				end
			end
			if k == "Parent" then
				return game
			end
			error(tostring(k) .. " is not a valid member of " .. name, 0)
		end,
		__newindex = function(_, k, v)
			if (def.props and def.props[k] ~= nil) or values[k] ~= nil then
				values[k] = v
				return
			end
			error(tostring(k) .. " is not a valid member of " .. name, 0)
		end,
		__tostring = function()
			return name
		end,
	})
	made[name] = obj
	return obj
end

------------------------------------------------------------------------------
-- Looking members up
------------------------------------------------------------------------------

local enum_default = {}
for _, group in pairs(props) do
	for k, v in pairs(group) do
		if type(v) == "string" and v:find("^%u%a+%.%w+$") then
			enum_default[v] = true
		end
	end
end

-- (found, value) for member `key` of `obj`, whose groups are `groups`.
function L.index(key_id, obj, key, groups)
	local s = slot(key_id)
	for _, group in ipairs(groups) do
		local get = getters[group] and getters[group][key]
		if get then
			return true, get(obj, s)
		end
	end
	local v = s.values[key]
	if v ~= nil then
		if v == NIL then
			return true, nil
		end
		return true, v
	end
	for _, group in ipairs(groups) do
		local p = props[group] and props[group][key]
		if p ~= nil then
			if p == NIL then
				return true, nil
			end
			if enum_default[p] then
				return true, resolve(p)
			end
			return true, p
		end
	end
	local sig = s.signals[key]
	if sig then
		return true, sig
	end
	for _, group in ipairs(groups) do
		local link = signals[group] and signals[group][key]
		if link then
			sig = new_signal()
			s.signals[key] = sig
			if type(link) == "function" then
				link(obj, sig, s)
			end
			return true, sig
		end
	end
	for _, group in ipairs(groups) do
		local m = methods[group] and methods[group][key]
		if m then
			return true, function(self, ...)
				CURRENT = s
				return m(self, ...)
			end
		end
	end
	return false
end

function L.new_index(key_id, obj, key, value, groups)
	local s = slot(key_id)
	for _, group in ipairs(groups) do
		local set = setters[group] and setters[group][key]
		if set then
			set(obj, value, s)
			return true
		end
	end
	local known = s.values[key] ~= nil
	if not known then
		for _, group in ipairs(groups) do
			if (props[group] and props[group][key] ~= nil) or (getters[group] and getters[group][key]) then
				known = true
				break
			end
		end
	end
	if known then
		s.values[key] = value == nil and NIL or value
		return true
	end
	return false
end

function L.forget(key_id)
	store[key_id] = nil
end

------------------------------------------------------------------------------
-- Globals
------------------------------------------------------------------------------

function L.globals()
	State = Enum.HumanoidStateType
	local shared_table = {}
	g.shared = shared_table
	g._G = shared_table
	g.Game = game
	g.Workspace = workspace
	g.ypcall = pcall
	g.Wait = g.wait
	g.Spawn = g.spawn
	g.Delay = g.delay
	g.version = function()
		return "0.1.0"
	end
	g.printidentity = function(prefix)
		print((prefix or "Current identity is") .. " 2")
	end

	local function box(min, max)
		return { CFrame = CFrame.new((min + max) / 2), Size = max - min }
	end
	local Region3Meta = {}
	Region3Meta.__index = Region3Meta
	Region3Meta.__tostring = function(r)
		return tostring(r.CFrame.Position) .. "; " .. tostring(r.Size)
	end
	function Region3Meta:ExpandToGrid(resolution)
		local half = self.Size / 2
		local lo, hi = self.CFrame.Position - half, self.CFrame.Position + half
		local function down(v)
			return math.floor(v / resolution) * resolution
		end
		local function up(v)
			return math.ceil(v / resolution) * resolution
		end
		return setmetatable(
			box(Vector3.new(down(lo.X), down(lo.Y), down(lo.Z)), Vector3.new(up(hi.X), up(hi.Y), up(hi.Z))),
			Region3Meta
		)
	end
	g.Region3 = table.freeze({
		new = function(min, max)
			min, max = min or Vector3.zero, max or Vector3.zero
			return setmetatable(box(min, max), Region3Meta)
		end,
	})
	g.Region3int16 = g.Region3
	local function int16(x, y, z)
		return Vector3.new(math.floor(x or 0), math.floor(y or 0), math.floor(z or 0))
	end
	g.Vector3int16 = table.freeze({ new = int16 })
	g.Vector2int16 = table.freeze({
		new = function(x, y)
			return Vector2.new(math.floor(x or 0), math.floor(y or 0))
		end,
	})

	local RectMeta = {}
	RectMeta.__index = function(r, k)
		if k == "Width" then
			return r.Max.X - r.Min.X
		elseif k == "Height" then
			return r.Max.Y - r.Min.Y
		end
	end
	RectMeta.__tostring = function(r)
		return tostring(r.Min) .. ", " .. tostring(r.Max)
	end
	g.Rect = table.freeze({
		new = function(a, b, c, d)
			if typeof(a) == "Vector2" then
				return setmetatable({ Min = a, Max = b }, RectMeta)
			end
			return setmetatable({ Min = Vector2.new(a or 0, b or 0), Max = Vector2.new(c or 0, d or 0) }, RectMeta)
		end,
	})

	local function flags(names)
		return function(...)
			local t = {}
			for _, n in ipairs(names) do
				t[n] = false
			end
			for _, v in ipairs({ ... }) do
				local n = typeof(v) == "string" and v or v.Name
				t[n] = true
			end
			return t
		end
	end
	g.Faces = table.freeze({ new = flags({ "Top", "Bottom", "Left", "Right", "Back", "Front" }) })
	g.Axes = table.freeze({ new = flags({ "X", "Y", "Z", "Top", "Bottom", "Left", "Right", "Back", "Front" }) })

	local FontMeta = {}
	FontMeta.__index = FontMeta
	FontMeta.__tostring = function(f)
		return "Font { Family = " .. f.Family .. " }"
	end
	local function font(family, weight, style)
		local f = setmetatable({
			Family = family,
			Weight = weight or Enum.FontWeight.Regular,
			Style = style or Enum.FontStyle.Normal,
		}, FontMeta)
		f.Bold = f.Weight.Value >= 600
		return f
	end
	g.Font = table.freeze({
		new = font,
		fromEnum = function(e)
			return font("libroxel://fonts/" .. e.Name)
		end,
		fromName = function(name, weight, style)
			return font("libroxel://fonts/" .. name, weight, style)
		end,
		fromId = function(id, weight, style)
			return font("libroxel://fonts/" .. tostring(id), weight, style)
		end,
	})

	local settings_tree = {
		Rendering = { QualityLevel = 21, MeshPartDetailLevel = NIL, EditQualityLevel = 21 },
		Physics = { AllowSleep = true, PhysicsEnvironmentalThrottle = NIL, AreOwnersShown = false },
		Network = { IncomingReplicationLag = 0 },
		Diagnostics = {},
	}
	g.settings = function()
		return settings_tree
	end
	local user_game_settings = {
		MasterVolume = 1,
		SavedQualityLevel = NIL,
		ComputerMovementMode = NIL,
		ComputerCameraMovementMode = NIL,
		MouseSensitivity = 1,
		RotationType = NIL,
		GamepadCameraSensitivity = 1,
		Fullscreen = false,
	}
	user_game_settings.GetCameraYInvertValue = function()
		return 1
	end
	user_game_settings.InFullScreen = function()
		return false
	end
	user_game_settings.InStudioMode = function()
		return false
	end
	user_game_settings.FullscreenChanged = new_signal()
	local user_settings = {
		GetService = function(_, name)
			if name == "UserGameSettings" then
				return user_game_settings
			end
			error(tostring(name) .. " is not a valid service name", 0)
		end,
		IsUserFeatureEnabled = function()
			return false
		end,
	}
	g.UserSettings = function()
		return user_settings
	end
end

return L
