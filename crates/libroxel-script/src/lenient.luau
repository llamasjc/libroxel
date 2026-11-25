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
	-- (`connected` and `disconnect` are the older spellings.)
	local conn = { Connected = true, connected = true }
	local handler = { fn = fn, conn = conn }
	table.insert(self.handlers, handler)
	conn.Disconnect = function()
		conn.Connected = false
		conn.connected = false
		local i = table.find(self.handlers, handler)
		if i then
			table.remove(self.handlers, i)
		end
	end
	conn.disconnect = conn.Disconnect
	return conn
end
Signal.ConnectParallel = Signal.Connect
Signal.connect = Signal.Connect

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
Signal.wait = Signal.Wait

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
	SwipeDirection = { "Right", "Left", "Up", "Down", "None" },
	VibrationMotor = { "Large", "Small", "LeftTrigger", "RightTrigger", "LeftHand", "RightHand" },
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

local function item_list(t)
	local list = {}
	for k, v in pairs(t) do
		if type(v) ~= "function" and (type(v) == "table" or type(v) == "userdata") then
			table.insert(list, v)
		end
	end
	return list
end

-- `database`: every enumeration the reference platform has, as name -> { {item, number} }.
function L.add_enums(lib, database)
	for name, items in pairs(ENUMS) do
		if rawget(lib, name) == nil then
			rawset(lib, name, make_enum(name, items))
		end
	end
	for name, items in pairs(database or {}) do
		local existing = rawget(lib, name)
		if existing == nil then
			rawset(lib, name, make_enum(name, items))
		elseif getmetatable(existing) == nil then
			-- One of libroxel's: add the items it doesn't have, so scripts
			-- naming them still run (they act like its first item).
			local stand_for = setmetatable({}, {
				__tostring = function()
					return name
				end,
			})
			for _, pair in ipairs(items) do
				if rawget(existing, pair[1]) == nil then
					rawset(existing, pair[1], setmetatable({ Name = pair[1], Value = pair[2], EnumType = stand_for }, ItemMeta))
				end
			end
		end
	end
	-- Every enumeration, as a list (`Enum:GetEnums()`).
	if rawget(lib, "GetEnums") == nil then
		rawset(lib, "GetEnums", function()
			local list = {}
			for _, t in pairs(lib) do
				if type(t) == "table" then
					table.insert(list, t)
				end
			end
			table.sort(list, function(a, b)
				return tostring(a) < tostring(b)
			end)
			return list
		end)
	end
	-- Looking items up by name or number, on every enumeration.
	for name, t in pairs(lib) do
		if type(t) == "table" and getmetatable(t) == nil then
			if rawget(t, "FromName") == nil then
				rawset(t, "FromName", function(_, n)
					local v = rawget(t, n)
					if type(v) == "function" then
						return nil
					end
					return v
				end)
			end
			if rawget(t, "FromValue") == nil then
				rawset(t, "FromValue", function(_, v)
					for _, it in ipairs(item_list(t)) do
						if it.Value == v then
							return it
						end
					end
					return nil
				end)
			end
		end
	end
	-- libroxel's own enumerations print their names, as the others do (an
	-- item's EnumType is its enumeration).
	for name, t in pairs(lib) do
		if type(t) == "table" and getmetatable(t) == nil then
			setmetatable(t, {
				__tostring = function()
					return name
				end,
			})
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

-- The reference platform's top bar: screen GUIs start below it unless they ignore the inset.
local GUI_INSET = 58
-- Where a screen GUI's area starts (measured from below the top bar) and how big it is.
local function screen_rect(layer)
	local vp = viewport()
	local ignore = false
	if layer then
		local ok, v = pcall(function()
			return layer.IgnoreGuiInset
		end)
		ignore = ok and v == true
		local ok2, insets = pcall(function()
			return layer.ScreenInsets
		end)
		if ok2 and insets ~= nil then
			local n = typeof(insets) == "EnumItem" and insets.Name or tostring(insets)
			if string.find(n, "None", 1, true) or string.find(n, "DeviceSafeInsets", 1, true) then
				ignore = true
			end
		end
	end
	if ignore then
		return Vector2.new(0, -GUI_INSET), vp
	end
	return Vector2.zero, Vector2.new(vp.X, math.max(0, vp.Y - GUI_INSET))
end
L.screen_rect = screen_rect
local function layer_of(obj)
	local at = obj
	while at ~= nil and not at:IsA("LayerCollector") do
		at = at.Parent
	end
	return at
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
		local _, area = screen_rect(layer_of(obj))
		within = area
	end
	if not ok or typeof(size) ~= "UDim2" then
		return within
	end
	return Vector2.new(size.X.Scale * within.X + size.X.Offset, size.Y.Scale * within.Y + size.Y.Offset)
end

local function abs_position(obj)
	local parent = obj.Parent
	local origin, within
	if parent and parent:IsA("GuiObject") then
		origin, within = abs_position(parent), abs_size(parent)
	else
		origin, within = screen_rect(layer_of(obj))
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
	local moving = L.moving(h)
	if moving then
		return State[moving]
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
		-- Climbing tells how fast it goes up; swimming, how fast it goes.
		local along = now == State.Climbing and sig.Climbing or now == State.Swimming and sig.Swimming
		if along then
			local v = root and velocity(root) or Vector3.zero
			local rate = now == State.Climbing and v.Y or v.Magnitude
			if math.abs(rate - last_speed) > 0.1 then
				last_speed = rate
				along:Fire(rate)
			end
		elseif sig.Running and math.abs(speed - last_speed) > 0.1 then
			last_speed = speed
			sig.Running:Fire(speed)
		end
	end)
end

------------------------------------------------------------------------------
-- What each kind of object gets. Groups are class names and families
-- (BasePart, GuiObject, GuiButton, ...); "Instance" is everything.
------------------------------------------------------------------------------

local editable_keys = setmetatable({}, { __mode = "k" }) -- EditableImage -> its key (see editable_image)
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
	LocalTransparencyModifier = 0,
	RootPriority = 0,
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
	CustomPhysicalProperties = function(self)
		return L.get_physical(self)
	end,
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
	CustomPhysicalProperties = function(self, v)
		L.set_physical(self, v)
	end,
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
		-- Like the reference platform: parts, and everything that places things relative to
		-- them (joint offsets, attachments, meshes, hip height), scale together,
		-- so a scaled rig keeps its shape when animated.
		local function scaled(cf)
			return CFrame.new(cf.Position * by) * (CFrame.new(cf.Position):Inverse() * cf)
		end
		for _, d in ipairs(self:GetDescendants()) do
			if d:IsA("BasePart") then
				local cf = d.CFrame
				local turn = CFrame.new(cf.Position):Inverse() * cf
				d.Size *= by
				d.CFrame = CFrame.new(pivot + (cf.Position - pivot) * by) * turn
			elseif d:IsA("JointInstance") then
				pcall(function()
					d.C0 = scaled(d.C0)
					d.C1 = scaled(d.C1)
				end)
			elseif d:IsA("Attachment") then
				pcall(function()
					d.Position *= by
				end)
			elseif d:IsA("SpecialMesh") or d:IsA("BlockMesh") or d:IsA("CylinderMesh") then
				pcall(function()
					d.Scale *= by
					d.Offset *= by
				end)
			elseif d:IsA("Humanoid") then
				pcall(function()
					d.HipHeight *= by
				end)
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
	Climbing = state_signal,
	GettingUp = true,
	Swimming = state_signal,
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
add(setters, "GuiObject", {
	-- A picture by address, or one a script draws (an EditableImage).
	ImageContent = function(self, v, s)
		s.values.ImageContent = v
		local key = type(v) == "table" and v.Object and editable_keys[v.Object]
		if key then
			self.Image = "editable://" .. key
		else
			self.Image = type(v) == "table" and v.Uri or ""
		end
	end,
})
add(getters, "GuiObject", {
	ImageContent = function(self, s)
		local v = s.values.ImageContent
		if v ~= nil then
			return v
		end
		return { SourceType = "Uri", Uri = self.Image }
	end,
	AbsoluteSize = function(self)
		local _, _, w, h = L.drawn_rect(self)
		if w then
			return Vector2.new(w, h)
		end
		return abs_size(self)
	end,
	AbsolutePosition = function(self)
		local x, y = L.drawn_rect(self)
		if x then
			return Vector2.new(x, y)
		end
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
add(signals, "GuiButton", {
	MouseButton1Down = true,
	MouseButton1Up = true,
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
	AbsoluteSize = function(self)
		local _, size = screen_rect(self)
		return size
	end,
	AbsolutePosition = function(self)
		local origin = screen_rect(self)
		return origin
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
-- A scrolling frame's canvas in pixels: its CanvasSize, grown to hold what's
-- in it along the axes AutomaticCanvasSize names, and never smaller than
-- the frame (as the renderer draws it).
add(getters, "ScrollingFrame", {
	AbsoluteCanvasSize = function(self)
		local frame = abs_size(self)
		local canvas = self.CanvasSize
		local w = canvas.X.Scale * frame.X + canvas.X.Offset
		local h = canvas.Y.Scale * frame.Y + canvas.Y.Offset
		local auto = tostring(self.AutomaticCanvasSize)
		if auto ~= "Enum.AutomaticSize.None" then
			local layout = self:FindFirstChildOfClass("UIListLayout") or self:FindFirstChildOfClass("UIGridLayout")
			local extent = layout and layout.AbsoluteContentSize or Vector2.zero
			if not layout then
				for _, child in ipairs(self:GetChildren()) do
					if child:IsA("GuiObject") and child.Visible then
						local size = abs_size(child)
						local at = child.Position
						extent = Vector2.new(
							math.max(extent.X, at.X.Scale * frame.X + at.X.Offset + size.X),
							math.max(extent.Y, at.Y.Scale * frame.Y + at.Y.Offset + size.Y)
						)
					end
				end
			end
			if auto == "Enum.AutomaticSize.X" or auto == "Enum.AutomaticSize.XY" then
				w = math.max(w, extent.X)
			end
			if auto == "Enum.AutomaticSize.Y" or auto == "Enum.AutomaticSize.XY" then
				h = math.max(h, extent.Y)
			end
		end
		return Vector2.new(math.max(w, frame.X), math.max(h, frame.Y))
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
-- The time of day as "hh:mm:ss" text: another way to read and set ClockTime.
add(getters, "Lighting", {
	TimeOfDay = function(self)
		local seconds = math.floor((self.ClockTime or 14) % 24 * 3600 + 0.5) % 86400
		return string.format("%02d:%02d:%02d", seconds // 3600, seconds // 60 % 60, seconds % 60)
	end,
})
add(setters, "Lighting", {
	TimeOfDay = function(self, v)
		local h, m, sec = string.match(tostring(v), "^%s*(%-?%d+):?(%d*):?(%d*)")
		if not h then
			return
		end
		local hours = tonumber(h) + (tonumber(m) or 0) / 60 + (tonumber(sec) or 0) / 3600
		self.ClockTime = hours % 24
	end,
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
add(setters, "Motor6D", {
	-- The animated offset between C0 and C1: libroxel's joints use it too.
	Transform = function(self, v, s)
		s.values.Transform = v
		local p = v.Position
		local rx, ry, rz = v:ToOrientation()
		L.set_joint_transform(self, p.X, p.Y, p.Z, math.deg(rx), math.deg(ry), math.deg(rz))
	end,
})
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
add(methods, "ParticleEmitter", { Clear = function() end, FastForward = function() end })
-- Things that load first: everything is loaded up front, so these have
-- nothing to wait for.
add(methods, "ReplicatedStorage", {
	RemoveDefaultLoadingScreen = function() end,
	SetDefaultLoadingGuiScreen = function() end,
	IsFinishedReplicating = function()
		return true
	end,
})
plain_signals("ReplicatedStorage", { "FinishedReplicating" })
-- Actors: messages go to the handlers bound for the topic, each in its own thread.
add(methods, "Actor", {
	BindToMessage = function(_, topic, fn)
		local s = CURRENT
		s.topics = s.topics or {}
		s.topics[topic] = s.topics[topic] or {}
		local list = s.topics[topic]
		table.insert(list, fn)
		local conn = { Connected = true }
		conn.Disconnect = function()
			conn.Connected = false
			local i = table.find(list, fn)
			if i then
				table.remove(list, i)
			end
		end
		return conn
	end,
	SendMessage = function(_, topic, ...)
		local list = CURRENT.topics and CURRENT.topics[topic]
		-- On the reference platform handlers run on the Actor's own core: never held back for the frame.
		for _, fn in ipairs(list and table.clone(list) or {}) do
			task.__parallel_defer(fn, ...)
		end
	end,
})
methods.Actor.BindToMessageParallel = methods.Actor.BindToMessage
-- The Actor a script runs under, if any.
local function get_actor(self)
	local at = self.Parent
	while at ~= nil do
		if at.ClassName == "Actor" then
			return at
		end
		at = at.Parent
	end
	return nil
end
for _, class in { "BaseScript", "Script", "LocalScript", "ModuleScript" } do
	add(methods, class, { GetActor = get_actor })
end
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
		local values = CURRENT.values
		if kind == Enum.CoreGuiType.All then
			-- All sets every kind at once (later single changes still apply).
			for k in pairs(values) do
				if type(k) == "string" and string.sub(k, 1, 8) == "coregui:" then
					values[k] = nil
				end
			end
		end
		values["coregui:" .. tostring(kind)] = enabled
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
	CreatorType = "CreatorType.User",
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

-- Functions bound to the render step run each frame in priority order
-- (lowest first, then in the order they were bound), from one connection.
local render_steps = {}
local render_connection
local render_order = 0
local function run_render_steps(dt)
	local list = {}
	for _, step in pairs(render_steps) do
		table.insert(list, step)
	end
	table.sort(list, function(a, b)
		if a.priority == b.priority then
			return a.order < b.order
		end
		return a.priority < b.priority
	end)
	for _, step in ipairs(list) do
		if render_steps[step.name] == step then
			local ok, err = pcall(step.fn, dt)
			if not ok then
				task.spawn(error, err, 0)
			end
		end
	end
end
add(methods, "RunService", {
	BindToRenderStep = function(self, name, priority, fn)
		if type(name) ~= "string" then
			error("BindToRenderStep expects a name", 2)
		end
		if type(fn) ~= "function" then
			error("BindToRenderStep expects a function", 2)
		end
		if typeof(priority) == "EnumItem" then
			priority = priority.Value
		end
		render_order += 1
		render_steps[name] = { name = name, priority = tonumber(priority) or 0, order = render_order, fn = fn }
		if not render_connection then
			render_connection = self.RenderStepped:Connect(run_render_steps)
		end
	end,
	UnbindFromRenderStep = function(_, name)
		render_steps[name] = nil
		if next(render_steps) == nil and render_connection then
			render_connection:Disconnect()
			render_connection = nil
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

-- Ordered data stores, kept in one value of an ordinary data store. As
-- documented, they hold only whole numbers, and sort by value.
local function ordered_store(name, scope)
	local backing = service("DataStoreService"):GetDataStore(name .. " (ordered)", scope)
	local KEY = "values"
	local store = {}
	local function all()
		return backing:GetAsync(KEY) or {}
	end
	local function check_key(key)
		if type(key) ~= "string" or #key == 0 or #key > 50 then
			error("a key must be 1 to 50 characters", 3)
		end
	end
	local function whole(value, what)
		if type(value) ~= "number" or value ~= value or math.floor(value) ~= value then
			error(what .. " must be a whole number in an ordered data store", 3)
		end
		return value
	end
	function store.GetAsync(_, key)
		check_key(key)
		return all()[key]
	end
	function store.SetAsync(_, key, value)
		check_key(key)
		local t = all()
		t[key] = whole(value, "the value")
		backing:SetAsync(KEY, t)
	end
	function store.IncrementAsync(_, key, by)
		check_key(key)
		local t = all()
		if t[key] ~= nil and type(t[key]) ~= "number" then
			error(key .. " does not hold a number", 2)
		end
		t[key] = (t[key] or 0) + whole(by or 1, "the increment")
		backing:SetAsync(KEY, t)
		return t[key]
	end
	function store.UpdateAsync(_, key, fn)
		check_key(key)
		local t = all()
		local new = fn(t[key])
		if new ~= nil then
			t[key] = whole(new, "the value")
			backing:SetAsync(KEY, t)
		end
		return t[key]
	end
	function store.RemoveAsync(_, key)
		check_key(key)
		local t = all()
		local old = t[key]
		t[key] = nil
		backing:SetAsync(KEY, t)
		return old
	end
	function store.GetSortedAsync(_, ascending, size, min, max)
		if type(ascending) ~= "boolean" then
			error("GetSortedAsync expects whether to sort ascending first", 2)
		end
		size = size or 50
		if size < 1 or size > 100 then
			error("a page holds 1 to 100 entries", 2)
		end
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
		return L.list_pages(rows, size)
	end
	return setmetatable(store, {
		__index = function(_, k)
			error(tostring(k) .. " is not a valid member of OrderedDataStore", 2)
		end,
		__tostring = function()
			return name
		end,
	})
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

local NOOP = function() end
local function returns(...)
	local v = table.pack(...)
	return function()
		return table.unpack(v, 1, v.n)
	end
end

-- Pictures scripts draw pixel by pixel. The pixels live here (so reading
-- them back works); once a frame, changed ones go to the renderer, which
-- shows them where an image's ImageContent names them.
local dirty_editables = {} -- key -> function that sends the pixels
function L.flush_editables()
	if next(dirty_editables) == nil then
		return
	end
	local list = dirty_editables
	dirty_editables = {}
	for _, send in pairs(list) do
		send()
	end
end
local function editable_image(size)
	size = size or Vector2.new(512, 512)
	local w, h = math.max(1, math.floor(size.X)), math.max(1, math.floor(size.Y))
	if w > 4096 or h > 4096 then
		error("an EditableImage can be at most 4096 by 4096", 3)
	end
	local pixels = buffer.create(w * h * 4)
	local gone = false
	local key = L.editable_key()
	local function send()
		L.editable_store(key, w, h, if gone then nil else pixels)
	end
	local function check()
		if gone then
			error("the EditableImage was destroyed", 3)
		end
		dirty_editables[key] = send
	end
	local function rect(pos, sz)
		local x0, y0 = math.max(0, math.floor(pos.X)), math.max(0, math.floor(pos.Y))
		local x1, y1 = math.min(w, x0 + math.floor(sz.X)), math.min(h, y0 + math.floor(sz.Y))
		return x0, y0, x1, y1
	end
	local function rgba(color, transparency)
		local a = math.floor((1 - (transparency or 0)) * 255 + 0.5)
		return math.floor(color.R * 255 + 0.5) + math.floor(color.G * 255 + 0.5) * 256
			+ math.floor(color.B * 255 + 0.5) * 65536 + a * 16777216
	end
	local img
	img = L.stand_in("EditableImage", {
		props = { Size = Vector2.new(w, h), Name = "EditableImage" },
		methods = {
			Destroy = function()
				gone = true
				dirty_editables[key] = send
			end,
			WritePixelsBuffer = function(_, pos, sz, buf)
				check()
				local x0, y0, x1, y1 = rect(pos, sz)
				local sw = math.floor(sz.X)
				for y = y0, y1 - 1 do
					local from = ((y - y0) * sw) * 4
					local n = (x1 - x0) * 4
					if from + n <= buffer.len(buf) then
						buffer.copy(pixels, (y * w + x0) * 4, buf, from, n)
					end
				end
			end,
			ReadPixelsBuffer = function(_, pos, sz)
				check()
				local x0, y0, x1, y1 = rect(pos, sz)
				local sw = math.floor(sz.X)
				local out = buffer.create(math.max(0, sw * math.floor(sz.Y) * 4))
				for y = y0, y1 - 1 do
					buffer.copy(out, ((y - y0) * sw) * 4, pixels, (y * w + x0) * 4, (x1 - x0) * 4)
				end
				return out
			end,
			WritePixels = function(_, pos, sz, list)
				check()
				local x0, y0, x1, y1 = rect(pos, sz)
				local sw = math.floor(sz.X)
				for y = y0, y1 - 1 do
					for x = x0, x1 - 1 do
						local i = ((y - y0) * sw + (x - x0)) * 4
						for c = 0, 3 do
							local v = list[i + c + 1] or 0
							buffer.writeu8(pixels, (y * w + x) * 4 + c, math.clamp(math.floor(v * 255 + 0.5), 0, 255))
						end
					end
				end
			end,
			ReadPixels = function(_, pos, sz)
				check()
				local x0, y0, x1, y1 = rect(pos, sz)
				local out = {}
				for y = y0, y1 - 1 do
					for x = x0, x1 - 1 do
						for c = 0, 3 do
							table.insert(out, buffer.readu8(pixels, (y * w + x) * 4 + c) / 255)
						end
					end
				end
				return out
			end,
			DrawRectangle = function(_, pos, sz, color, transparency)
				check()
				local x0, y0, x1, y1 = rect(pos, sz)
				local v = rgba(color, transparency)
				for y = y0, y1 - 1 do
					for x = x0, x1 - 1 do
						buffer.writeu32(pixels, (y * w + x) * 4, v)
					end
				end
			end,
			DrawCircle = function(_, center, radius, color, transparency)
				check()
				local v = rgba(color, transparency)
				local r2 = radius * radius
				for y = math.max(0, math.floor(center.Y - radius)), math.min(h - 1, math.ceil(center.Y + radius)) do
					for x = math.max(0, math.floor(center.X - radius)), math.min(w - 1, math.ceil(center.X + radius)) do
						local dx, dy = x - center.X, y - center.Y
						if dx * dx + dy * dy <= r2 then
							buffer.writeu32(pixels, (y * w + x) * 4, v)
						end
					end
				end
			end,
			DrawLine = function(_, a, b, color, transparency)
				check()
				local v = rgba(color, transparency)
				local steps = math.max(math.abs(b.X - a.X), math.abs(b.Y - a.Y), 1)
				for i = 0, steps do
					local x = math.floor(a.X + (b.X - a.X) * i / steps + 0.5)
					local y = math.floor(a.Y + (b.Y - a.Y) * i / steps + 0.5)
					if x >= 0 and y >= 0 and x < w and y < h then
						buffer.writeu32(pixels, (y * w + x) * 4, v)
					end
				end
			end,
			DrawImage = function() end,
			DrawImageTransformed = function() end,
			DrawImageProjected = function() end,
			DrawTriangle = function() end,
			Crop = function() end,
			Resize = function() end,
			Rotate = function() end,
			Copy = function()
				return img
			end,
			IsA = function(_, c)
				return c == "EditableImage" or c == "Object"
			end,
		},
	}, "editable:" .. key)
	editable_keys[img] = key
	return img
end
L.editable_image = editable_image
local function editable_mesh()
	return L.stand_in("EditableMesh", {
		props = { Name = "EditableMesh", SkinningEnabled = false },
		methods = {
			Destroy = function() end,
			AddVertex = function() return 1 end,
			AddTriangle = function() return 1 end,
			GetVertices = function() return {} end,
			GetFaces = function() return {} end,
		},
	}, "editablemesh:" .. tostring(math.random()))
end

local SERVICES = {
	GamepadService = {
		props = { GamepadCursorEnabled = false },
		signals = { "GamepadThumbstick1Changed" },
		methods = {
			EnableGamepadCursor = NOOP,
			DisableGamepadCursor = NOOP,
		},
	},
	-- The reference platform's own menus live here; scripts can't put things in it there
	-- either, so it only needs to exist.
	CoreGui = { props = { SelectionImageObject = NIL } },
	AnimationClipProvider = {
		methods = {
			GetAnimationClipAsync = NOOP,
			RegisterAnimationClip = returns(""),
		},
	},
	ScriptContext = {
		signals = { "Error" },
		methods = { SetTimeout = NOOP },
	},
	ExperienceService = {},
	AvatarEditorService = {
		signals = { "PromptSaveAvatarCompleted", "PromptAllowInventoryReadAccessCompleted" },
		methods = {
			PromptSaveAvatar = NOOP,
			PromptAllowInventoryReadAccess = NOOP,
			GetItemDetails = function()
				return {}
			end,
		},
	},
	ChangeHistoryService = {
		signals = { "OnUndo", "OnRedo" },
		methods = {
			SetWaypoint = NOOP,
			TryBeginRecording = returns(""),
			FinishRecording = NOOP,
			ResetWaypoints = NOOP,
		},
	},
	Selection = {
		signals = { "SelectionChanged" },
		methods = {
			Get = function()
				return {}
			end,
			Set = NOOP,
			Add = NOOP,
			Remove = NOOP,
		},
	},
	TextChatService = {
		props = {
			ChatVersion = "ChatVersion.TextChatService",
			CreateDefaultTextChannels = true,
			CreateDefaultCommands = true,
			OnIncomingMessage = NIL,
		},
		signals = { "MessageReceived", "SendingMessage", "BubbleDisplayed" },
		getters = {
			ChatWindowConfiguration = function()
				return L.chat_config("ChatWindowConfiguration", {
					Enabled = true, BackgroundColor3 = Color3.fromRGB(25, 27, 29), BackgroundTransparency = 0.3,
					TextColor3 = Color3.new(1, 1, 1), TextSize = 14, HeightScale = 1, WidthScale = 1,
					HorizontalAlignment = "HorizontalAlignment.Left", VerticalAlignment = "VerticalAlignment.Top",
					FontFace = NIL, TextStrokeTransparency = 0.5,
				})
			end,
			ChatInputBarConfiguration = function()
				return L.chat_config("ChatInputBarConfiguration", {
					Enabled = true, BackgroundColor3 = Color3.fromRGB(25, 27, 29), BackgroundTransparency = 0.2,
					TextColor3 = Color3.new(1, 1, 1), PlaceholderColor3 = Color3.fromRGB(178, 178, 178),
					KeyboardKeyCode = "KeyCode.Slash", TargetTextChannel = NIL, AutocompleteEnabled = true,
				})
			end,
			BubbleChatConfiguration = function()
				return L.chat_config("BubbleChatConfiguration", {
					Enabled = true, BubbleDuration = 15, MaxBubbles = 3, MaxDistance = 100, MinimizeDistance = 40,
					BackgroundColor3 = Color3.new(1, 1, 1), TextColor3 = Color3.fromRGB(57, 59, 61), TextSize = 16,
					VerticalStudsOffset = 0, BubblesSpacing = 6, TailVisible = true, LocalPlayerStudsOffset = Vector3.zero,
				})
			end,
			ChannelTabsConfiguration = function()
				return L.chat_config("ChannelTabsConfiguration", { Enabled = false })
			end,
			-- The usual channels and commands (made further down).
			TextChannels = function()
				return L.chat_text_channels()
			end,
			TextChatCommands = function()
				return L.chat_text_chat_commands()
			end,
		},
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
			FindFirstChild = function(_, name)
				return L.chat_child(name)
			end,
			WaitForChild = function(_, name)
				return L.chat_child(name)
			end,
			GetChildren = function()
				return L.chat_children()
			end,
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
		props = { SystemLocaleId = "en-us", RobloxLocaleId = "en-us" },
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
			CreateEditableImage = function(_, options)
				return editable_image(options and options.Size)
			end,
			CreateEditableImageAsync = function(_, _content, options)
				return editable_image(options and options.Size)
			end,
			CreateEditableMesh = function()
				return editable_mesh()
			end,
			CreateEditableMeshAsync = function()
				return editable_mesh()
			end,
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
			RegisterKeyframeSequence = function(_, sequence)
				return L.register_sequence(sequence)
			end,
			-- Only sequences registered here: assets aren't fetched.
			GetKeyframeSequenceAsync = function(_, id)
				local sequence = L.registered_sequence(tostring(id))
				if sequence == nil then
					error("libroxel can't fetch the keyframe sequence " .. tostring(id), 2)
				end
				return sequence:Clone()
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

local resolved = {}
local function resolve(v)
	-- Enum defaults are written "Kind.Item" and looked up when first read
	-- (then kept: games read these every frame, and matching the text each
	-- time was most of the cost).
	local known = resolved[v]
	if known ~= nil then
		return known
	end
	if type(v) == "string" and v:find("^%u%a+%.%w+$") then
		local kind, item = v:match("^(%a+)%.(%w+)$")
		local ok, e = pcall(function()
			return Enum[kind][item]
		end)
		if ok and e ~= nil then
			resolved[v] = e
			return e
		end
	end
	return v
end
-- More members for the services made in compat.rs.
local SERVICE_EXTRAS = {
	GuiService = {
		props = {
			MenuIsOpen = false,
			SelectedObject = NIL,
			AutoSelectGuiEnabled = true,
			GuiNavigationEnabled = true,
			CoreGuiNavigationEnabled = true,
			ReducedMotionEnabled = false,
			TouchControlsEnabled = true,
			PreferredTransparency = 1,
			PreferredTextSize = "PreferredTextSize.Medium",
		},
		getters = {
			TopbarInset = function()
				return Rect.new(0, 0, 0, 0)
			end,
		},
		signals = { "MenuOpened", "MenuClosed", "ErrorMessageChanged", "UiMessageChanged", "NativeClose" },
		methods = {
			IsWindows = returns(false),
			SetMenuIsOpen = NOOP,
			InspectPlayerFromUserId = NOOP,
			InspectPlayerFromHumanoidDescription = NOOP,
			CloseInspectMenu = NOOP,
			Select = NOOP,
			AddSelectionParent = NOOP,
			RemoveSelectionGroup = NOOP,
			GetEmotesMenuOpen = returns(false),
			SetEmotesMenuOpen = NOOP,
			GetInspectMenuEnabled = returns(false),
			SetInspectMenuEnabled = NOOP,
			OpenBrowserWindow = NOOP,
		},
	},
	ContextActionService = {
		signals = { "LocalToolEquipped", "LocalToolUnequipped" },
		methods = {
			SetTitle = NOOP,
			SetImage = NOOP,
			SetDescription = NOOP,
			SetPosition = NOOP,
			GetAllBoundActionInfo = function()
				return {}
			end,
			GetBoundActionInfo = function()
				return {}
			end,
			GetCurrentLocalToolIcon = returns(""),
			BindActivate = NOOP,
			UnbindActivate = NOOP,
		},
	},
	SoundService = {
		props = {
			AmbientReverb = "ReverbType.NoReverb",
			DistanceFactor = 3.33,
			DopplerScale = 1,
			RolloffScale = 1,
			RespectFilteringEnabled = true,
		},
		methods = {
			SetListener = NOOP,
			GetListener = function()
				return Enum.ListenerType.Camera, nil
			end,
		},
	},
	MarketplaceService = {
		signals = { "PromptPremiumPurchaseFinished", "ProcessReceiptFinished" },
		methods = {
			PromptPremiumPurchase = NOOP,
			PromptSubscriptionPurchase = NOOP,
			PromptBundlePurchase = NOOP,
			UserOwnsGamePassAsync = returns(false),
			GetDeveloperProductsAsync = function()
				return L.key_pages({})
			end,
			GetSubscriptionProductInfoAsync = function()
				return {}
			end,
		},
	},
	TeleportService = {
		signals = { "TeleportInitFailed", "LocalPlayerArrivedFromTeleport" },
		methods = {
			GetLocalPlayerTeleportData = NOOP,
			GetArrivingTeleportGui = NOOP,
			SetTeleportGui = NOOP,
			GetTeleportSetting = NOOP,
			SetTeleportSetting = NOOP,
			ReserveServer = returns("", ""),
			TeleportPartyAsync = NOOP,
		},
	},
	ContentProvider = {
		props = { RequestQueueSize = 0, BaseUrl = "" },
		methods = {
			GetAssetFetchStatus = function()
				return Enum.AssetFetchStatus.Success
			end,
		},
	},
	VRService = {
		props = { VREnabled = false, GuiInputUserCFrame = "UserCFrame.Head", AutomaticScaling = "VRScaling.World" },
		signals = { "UserCFrameChanged", "UserCFrameEnabled", "NavigationRequested" },
		methods = {
			GetUserCFrame = function()
				return CFrame.new()
			end,
			GetUserCFrameEnabled = returns(false),
			RecenterUserHeadCFrame = NOOP,
		},
	},
	-- On phones, the Touch motors run the phone's vibration motor (one
	-- motor, so Large and Small are the same); controllers have none here.
	HapticService = {
		methods = {
			IsMotorSupported = function(_, kind, motor)
				return kind == Enum.UserInputType.Touch
					and L.can_vibrate()
					and (motor == Enum.VibrationMotor.Large or motor == Enum.VibrationMotor.Small)
			end,
			SetMotor = function(_, kind, motor, value)
				if kind ~= Enum.UserInputType.Touch then
					return
				end
				L.motors = L.motors or {}
				L.motors[motor] = math.clamp(tonumber(value) or 0, 0, 1)
				local strongest = 0
				for _, v in L.motors do
					strongest = math.max(strongest, v)
				end
				L.set_vibration(strongest)
			end,
			GetMotor = function(_, kind, motor)
				if kind ~= Enum.UserInputType.Touch or not L.motors then
					return 0
				end
				return L.motors[motor] or 0
			end,
		},
	},
	TextService = {
		methods = {
			GetFamilyInfoAsync = function()
				return { Name = "", Faces = {} }
			end,
		},
	},
	PolicyService = {
		methods = {
			CanViewBrandProjectAsync = returns(false),
		},
	},
	BadgeService = {
		methods = {
			GetBadgeInfoAsync = function(_, id)
				return { Name = "", Description = "", IconImageId = 0, IsEnabled = false, Id = id }
			end,
			CheckUserBadgesAsync = function()
				return {}
			end,
		},
	},
}

-- (found, value) for member `key` of a compat.rs service called `name`.
function L.service_extra(name, key)
	local def = SERVICE_EXTRAS[name]
	local key_id = "service:" .. name
	if def then
		if def.methods and def.methods[key] then
			return true, def.methods[key]
		end
		if def.getters and def.getters[key] then
			return true, def.getters[key]()
		end
		local s = slot(key_id)
		local v = s.values[key]
		if v ~= nil then
			if v == NIL then
				return true, nil
			end
			return true, v
		end
		local p = def.props and def.props[key]
		if p ~= nil then
			if p == NIL then
				return true, nil
			end
			return true, resolve(p)
		end
		if def.signals and table.find(def.signals, key) then
			local sig = s.signals[key]
			if not sig then
				sig = new_signal()
				s.signals[key] = sig
			end
			return true, sig
		end
	end
	return L.common(key_id, name, key)
end

-- Sets member `key` of a compat.rs service: true if it's one it has.
function L.service_extra_set(name, key, value)
	local def = SERVICE_EXTRAS[name]
	local key_id = "service:" .. name
	local s = slot(key_id)
	if (def and def.props and def.props[key] ~= nil) or s.values[key] ~= nil then
		local stored = value == nil and NIL or value
		if s.values[key] ~= stored then
			s.values[key] = stored
			L.fire_changed(key_id, key)
		end
		return true
	end
	return false
end

-- An object that stands in for an instance libroxel has no class for:
-- `def` lists its props (defaults), getters, signals and methods. Setting a
-- prop fires its GetPropertyChangedSignal event.
local function stand_in(name, def, key_id)
	local values, sigs = {}, {}
	for _, n in ipairs(def.signals or {}) do
		sigs[n] = new_signal()
	end
	local obj = {}
	setmetatable(obj, {
		__index = function(_, k)
			if def.methods and def.methods[k] then
				return def.methods[k]
			end
			if def.getters and def.getters[k] then
				local v = def.getters[k]()
				values[k] = v
				return v
			end
			if sigs[k] then
				return sigs[k]
			end
			local v = values[k]
			if v ~= nil then
				if v == NIL then
					return nil
				end
				return v
			end
			local p = def.props and def.props[k]
			if p ~= nil then
				if p == NIL then
					return nil
				end
				return resolve(p)
			end
			local found, common = L.common(key_id, name, k)
			if found then
				return common
			end
			error(tostring(k) .. " is not a valid member of " .. name, 0)
		end,
		__newindex = function(_, k, v)
			if (def.props and def.props[k] ~= nil) or values[k] ~= nil then
				local stored = v == nil and NIL or v
				if values[k] ~= stored then
					values[k] = stored
					L.fire_changed(key_id, k)
				end
				return
			end
			error(tostring(k) .. " is not a valid member of " .. name, 0)
		end,
		__tostring = function()
			return name
		end,
		__type = name,
	})
	return obj
end
L.stand_in = stand_in

local configs = {}
function L.chat_config(name, props)
	if not configs[name] then
		configs[name] = stand_in(name, { props = props }, "chat:" .. name)
	end
	return configs[name]
end

-- Fires signal `signal` of a service made here (ProximityPromptService's
-- PromptTriggered and the like), if a script has asked for that service.
function L.fire_service_signal(name, signal, ...)
	local obj = L.service(name)
	local sig = obj and obj[signal]
	if sig then
		sig:Fire(...)
	end
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
	local obj = stand_in(name, def, "service:" .. name)
	made[name] = obj
	return obj
end


-- More of the reference platform's members ----------------------------------------------------

local function cell(v)
	return math.floor(v / 4)
end
add(methods, "Terrain", {
	WorldToCell = function(_, p)
		return Vector3.new(cell(p.X), cell(p.Y), cell(p.Z))
	end,
	WorldToCellPreferSolid = function(_, p)
		return Vector3.new(cell(p.X), cell(p.Y), cell(p.Z))
	end,
	WorldToCellPreferEmpty = function(_, p)
		return Vector3.new(cell(p.X), cell(p.Y), cell(p.Z))
	end,
	CellCenterToWorld = function(_, x, y, z)
		return Vector3.new(x * 4 + 2, y * 4 + 2, z * 4 + 2)
	end,
	CellCornerToWorld = function(_, x, y, z)
		return Vector3.new(x * 4, y * 4, z * 4)
	end,
	AutowedgeCell = function()
		return false
	end,
	AutowedgeCells = function() end,
	ConvertToSmooth = function() end,
})
add(getters, "Terrain", {
	MaxExtents = function()
		local big = 32000
		return Region3.new(Vector3.new(-big, -big, -big), Vector3.new(big, big, big))
	end,
})

add(methods, "Humanoid", {
	GetAppliedDescription = function(self)
		local d = Instance.new("HumanoidDescription")
		local model = self.Parent
		local colours = model and model:FindFirstChildOfClass("BodyColors")
		if colours then
			for _, limb in ipairs({ "Head", "Torso", "LeftArm", "RightArm", "LeftLeg", "RightLeg" }) do
				pcall(function()
					d[limb .. "Color"] = colours[limb .. "Color3"]
				end)
			end
		end
		return d
	end,
	ApplyDescription = function() end,
	ApplyDescriptionReset = function() end,
	ApplyDescriptionClientServer = function() end,
})

-- Making shapes from others while playing: libroxel can't carve parts, so
-- a union is one part over all of them, and a subtraction keeps the part.
local function bounds(parts)
	local lo, hi
	for _, p in ipairs(parts) do
		local half = p.Size / 2
		local c = p.Position
		local a, b = c - half, c + half
		lo = lo and Vector3.new(math.min(lo.X, a.X), math.min(lo.Y, a.Y), math.min(lo.Z, a.Z)) or a
		hi = hi and Vector3.new(math.max(hi.X, b.X), math.max(hi.Y, b.Y), math.max(hi.Z, b.Z)) or b
	end
	return lo, hi
end
add(methods, "BasePart", {
	UnionAsync = function(self, others)
		local all = { self }
		for _, o in ipairs(others or {}) do
			table.insert(all, o)
		end
		local lo, hi = bounds(all)
		local u = self:Clone()
		u.Name = "Union"
		u.Size = hi - lo
		u.CFrame = CFrame.new((lo + hi) / 2)
		return u
	end,
	SubtractAsync = function(self)
		local u = self:Clone()
		u.Name = "Union"
		return u
	end,
	IntersectAsync = function(self)
		local u = self:Clone()
		u.Name = "Intersection"
		return u
	end,
})

plain_signals("Dialog", { "DialogChoiceSelected" })

add(methods, "UIPageLayout", {
	Next = function() end,
	Previous = function() end,
	JumpTo = function() end,
	JumpToIndex = function() end,
})
plain_signals("UIPageLayout", { "PageEnter", "PageLeave", "Stopped" })

add(methods, "RunService", {
	IsRunning = function()
		return true
	end,
	IsStudio = function()
		return false
	end,
})

add(props, "Mouse", {
	TargetFilter = NIL,
	Icon = "",
	TargetSurface = "NormalId.Top",
	ViewSizeX = 0,
	ViewSizeY = 0,
})
add(getters, "Mouse", {
	UnitRay = function(self)
		local hit = self.Hit.Position
		local cam = workspace.CurrentCamera
		local origin = cam and cam.CFrame.Position or Vector3.zero
		return Ray.new(origin, (hit - origin).Unit)
	end,
	Origin = function()
		local cam = workspace.CurrentCamera
		return cam and cam.CFrame or CFrame.new()
	end,
})
plain_signals("Mouse", { "Button2Down", "Button2Up", "Move", "Idle", "WheelForward", "WheelBackward" })

-- Pages of `rows`, `size` to a page.
function L.list_pages(rows, size)
	size = math.max(1, size or 50)
	local page = 0
	local pages = { IsFinished = #rows <= size }
	function pages.GetCurrentPage()
		return table.move(rows, page * size + 1, math.min((page + 1) * size, #rows), 1, {})
	end
	function pages.AdvanceToNextPageAsync()
		if pages.IsFinished then
			return
		end
		page += 1
		pages.IsFinished = (page + 1) * size >= #rows
	end
	return pages
end

function L.key_pages(keys)
	local list = {}
	for _, k in ipairs(keys) do
		table.insert(list, { KeyName = k })
	end
	local pages = { IsFinished = true }
	function pages.GetCurrentPage()
		return list
	end
	function pages.AdvanceToNextPageAsync() end
	return pages
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


-- More members games reach for ----------------------------------------------

add(methods, "DataStoreService", {
	GetGlobalDataStore = function(self)
		return self:GetDataStore("global")
	end,
})
add(methods, "PlayerGui", {
	GetTopbarTransparency = function()
		return 0.5
	end,
	SetTopbarTransparency = function() end,
})
plain_signals("PlayerGui", { "TopbarTransparencyChangedSignal" })
add(methods, "TweenService", {
	-- Moves `current` toward `target` like a critically damped spring;
	-- returns the new value and velocity.
	SmoothDamp = function(_, current, target, velocity, smooth_time, max_speed, dt)
		smooth_time = math.max(smooth_time or 0.1, 1e-4)
		dt = dt or 1 / 60
		local omega = 2 / smooth_time
		local x = omega * dt
		local exp = 1 / (1 + x + 0.48 * x * x + 0.235 * x * x * x)
		local change = current - target
		local temp = (velocity + change * omega) * dt
		local new_velocity = (velocity - temp * omega) * exp
		return target + (change + temp) * exp, new_velocity
	end,
})
add(props, "Terrain", { Decoration = false, MaxExtents = NIL })
-- Everything is always loaded: streaming settings are kept but change nothing.
add(props, "Workspace", {
	StreamingEnabled = false,
	StreamingMinRadius = 64,
	StreamingTargetRadius = 1024,
	StreamingIntegrityMode = "StreamingIntegrityMode.Default",
	StreamOutBehavior = "StreamOutBehavior.Default",
	ModelStreamingBehavior = "ModelStreamingBehavior.Default",
	SignalBehavior = "SignalBehavior.Default",
	PhysicsSteppingMethod = "PhysicsSteppingMethod.Default",
	FluidForces = "FluidForces.Default",
	TouchesUseCollisionGroups = false,
	AirDensity = 0.0012,
	GlobalWind = Vector3.zero,
})
plain_signals("Workspace", { "PersistentLoaded" })
add(methods, "Workspace", {
	ArePartsTouchingOthers = function(_, parts)
		for _, p in ipairs(parts) do
			if #p:GetTouchingParts() > 0 then
				return true
			end
		end
		return false
	end,
	BulkMoveTo = function(_, parts, cframes)
		for i, p in ipairs(parts) do
			if cframes[i] then
				p.CFrame = cframes[i]
			end
		end
	end,
	JoinToOutsiders = function() end,
	UnjoinFromOutsiders = function() end,
	SetInsertPoint = function() end,
})
add(methods, "BasePart", {
	GetClosestPointOnSurface = function(self, point)
		-- The nearest point of the part's box.
		local local_point = self.CFrame:PointToObjectSpace(point)
		local half = self.Size / 2
		local clamped = Vector3.new(
			math.clamp(local_point.X, -half.X, half.X),
			math.clamp(local_point.Y, -half.Y, half.Y),
			math.clamp(local_point.Z, -half.Z, half.Z)
		)
		return self.CFrame:PointToWorldSpace(clamped)
	end,
	IsInRadius = function(self, point, radius)
		return (self.Position - point).Magnitude <= radius
	end,
})
plain_signals("Team", { "PlayerAdded", "PlayerRemoved" })
-- TextChatService ------------------------------------------------------------
-- Libroxel has no chat window, but games still talk through their channels:
-- system messages, chat tags set in OnIncomingMessage and slash commands.
-- Messages go round this computer the way the documentation says, and system
-- messages are also printed to the output.

plain_signals("TextChannel", { "MessageReceived" })
add(props, "TextChannel", { OnIncomingMessage = NIL, ShouldDeliverCallback = NIL })
plain_signals("TextChatCommand", { "Triggered" })
add(props, "TextChatCommand", { Enabled = true })

local chat = { folders = {}, sources = {}, next_id = 0 }

local function chat_service()
	return service("TextChatService")
end

-- The folders TextChatService keeps its channels and commands in. Made
-- once, so what a script connects to is what messages arrive on.
local function chat_folder(name, fill)
	local folder = chat.folders[name]
	if not folder then
		folder = Instance.new("Folder")
		folder.Name = name
		fill(folder)
		chat.folders[name] = folder
	end
	return folder
end

local DEFAULT_COMMANDS = {
	{ "RBXClearCommand", "/clear", "/cls" },
	{ "RBXConsoleCommand", "/console", "" },
	{ "RBXEmoteCommand", "/emote", "/e" },
	{ "RBXHelpCommand", "/help", "/?" },
	{ "RBXMuteCommand", "/mute", "/m" },
	{ "RBXTeamCommand", "/team", "/t" },
	{ "RBXUnmuteCommand", "/unmute", "/um" },
	{ "RBXVersionCommand", "/version", "/v" },
	{ "RBXWhisperCommand", "/whisper", "/w" },
}

local function text_channels()
	return chat_folder("TextChannels", function(folder)
		if chat_service().CreateDefaultTextChannels == false then
			return
		end
		for _, n in ipairs({ "RBXGeneral", "RBXSystem" }) do
			local c = Instance.new("TextChannel")
			c.Name = n
			c.Parent = folder
		end
	end)
end

local function text_chat_commands()
	return chat_folder("TextChatCommands", function(folder)
		if chat_service().CreateDefaultCommands == false then
			return
		end
		for _, d in ipairs(DEFAULT_COMMANDS) do
			local c = Instance.new("TextChatCommand")
			c.Name = d[1]
			c.PrimaryAlias = d[2]
			c.SecondaryAlias = d[3]
			c.Parent = folder
		end
	end)
end

local function chat_children()
	return { text_channels(), text_chat_commands() }
end

-- Who can speak in a channel (the engine makes these; scripts can't).
local function new_text_source(name, user_id)
	chat.next_id += 1
	return stand_in("TextSource", {
		props = { Name = name, UserId = user_id or 0, CanSend = true },
	}, "textsource:" .. chat.next_id)
end

-- The local player's TextSource in `channel`: who sent a message.
local function text_source(channel)
	local player = game:GetService("Players").LocalPlayer
	if not player then
		return nil
	end
	local source = chat.sources[channel]
	if not source then
		source = new_text_source(player.Name, player.UserId)
		chat.sources[channel] = source
	end
	return source
end

local function new_message(channel, text, source, metadata)
	chat.next_id += 1
	local id = service("HttpService"):GenerateGUID(false)
	return stand_in("TextChatMessage", {
		props = {
			Text = text,
			PrefixText = "",
			MessageId = id,
			Metadata = metadata or "",
			Status = Enum.TextChatMessageStatus.Success,
			TextChannel = channel,
			TextSource = source or NIL,
			Timestamp = DateTime.now(),
			Translation = "",
			BubbleChatMessageProperties = NIL,
		},
	}, "chatmsg:" .. id)
end

-- Runs an OnIncomingMessage callback and applies what it hands back.
local function incoming(callback, message)
	if type(callback) ~= "function" then
		return
	end
	local ok, props = pcall(callback, message)
	if not ok then
		task.spawn(error, props, 0)
		return
	end
	if props == nil then
		return
	end
	local function take(key)
		local ok2, v = pcall(function()
			return props[key]
		end)
		if ok2 and v ~= nil and v ~= "" then
			message[key] = v
		end
	end
	take("PrefixText")
	take("Text")
	take("Translation")
end

local function deliver(channel, message)
	incoming(channel.OnIncomingMessage, message)
	incoming(chat_service().OnIncomingMessage, message)
	channel.MessageReceived:Fire(message)
	chat_service().MessageReceived:Fire(message)
end

-- A slash command `text` names, if it is one.
local function find_command(text)
	local word = string.lower(string.match(text, "^(%S+)") or "")
	if string.sub(word, 1, 1) ~= "/" then
		return nil
	end
	for _, c in ipairs(text_chat_commands():GetDescendants()) do
		if c:IsA("TextChatCommand") and c.Enabled ~= false then
			local primary, secondary = string.lower(c.PrimaryAlias or ""), string.lower(c.SecondaryAlias or "")
			if (primary ~= "" and word == primary) or (secondary ~= "" and word == secondary) then
				return c
			end
		end
	end
	return nil
end

L.chat_text_channels = text_channels
L.chat_text_chat_commands = text_chat_commands
L.chat_children = chat_children
function L.chat_child(name)
	for _, c in ipairs(chat_children()) do
		if c.Name == name then
			return c
		end
	end
	return nil
end

add(methods, "TextChannel", {
	DisplaySystemMessage = function(self, text, metadata)
		text = tostring(text)
		print(text)
		local message = new_message(self, text, nil, metadata)
		deliver(self, message)
		return message
	end,
	SendAsync = function(self, text, metadata)
		text = tostring(text)
		local source = text_source(self)
		local message = new_message(self, text, source, metadata)
		message.Status = Enum.TextChatMessageStatus.Sending
		chat_service().SendingMessage:Fire(message)
		local command = find_command(text)
		if command then
			command.Triggered:Fire(source, text)
			message.Status = Enum.TextChatMessageStatus.Success
			return message
		end
		message.Status = Enum.TextChatMessageStatus.Success
		local player = game:GetService("Players").LocalPlayer
		if player then
			pcall(function()
				player.Chatted:Fire(text)
			end)
		end
		deliver(self, message)
		return message
	end,
	AddUserAsync = function(_, user_id)
		local player = game:GetService("Players"):GetPlayerByUserId(user_id)
		return new_text_source(player and player.Name or tostring(user_id), user_id), true
	end,
	SetDirectChatRequester = function() end,
})
add(methods, "CollectionService", {
	GetAllTags = function()
		local seen, list = {}, {}
		for _, d in ipairs(game:GetDescendants()) do
			for _, t in ipairs(d:GetTags()) do
				if not seen[t] then
					seen[t] = true
					table.insert(list, t)
				end
			end
		end
		return list
	end,
})
plain_signals("CollectionService", { "TagAdded", "TagRemoved" })
add(methods, "Path", {
	CheckOcclusionAsync = function()
		return -1
	end,
})
add(methods, "WorldModel", {
	Raycast = function() end,
	Blockcast = function() end,
	Spherecast = function() end,
	Shapecast = function() end,
	GetPartBoundsInBox = function()
		return {}
	end,
	GetPartBoundsInRadius = function()
		return {}
	end,
	GetPartsInPart = function()
		return {}
	end,
	BulkMoveTo = function(_, parts, cframes)
		for i, p in ipairs(parts) do
			if cframes[i] then
				p.CFrame = cframes[i]
			end
		end
	end,
})
add(props, "VideoFrame", { Playing = false, IsLoaded = true, TimeLength = 0, TimePosition = 0, Volume = 1, Looped = false })
add(methods, "VideoFrame", {
	Play = function(self)
		self.Playing = true
	end,
	Pause = function(self)
		self.Playing = false
	end,
})
plain_signals("VideoFrame", { "Played", "Paused", "Ended", "Loaded", "DidLoop" })
add(methods, "HumanoidDescription", {
	GetAccessories = function(_, include_rigid)
		return table.clone(CURRENT.values.__accessories or {})
	end,
	SetAccessories = function(_, list)
		CURRENT.values.__accessories = table.clone(list)
	end,
	GetEmotes = function()
		return table.clone(CURRENT.values.__emotes or {})
	end,
	SetEmotes = function(_, emotes)
		CURRENT.values.__emotes = table.clone(emotes)
	end,
	AddEmote = function(_, name, id)
		local e = CURRENT.values.__emotes or {}
		e[name] = e[name] or {}
		table.insert(e[name], id)
		CURRENT.values.__emotes = e
	end,
	RemoveEmote = function(_, name)
		local e = CURRENT.values.__emotes
		if e then
			e[name] = nil
		end
	end,
	GetEquippedEmotes = function()
		return table.clone(CURRENT.values.__equipped or {})
	end,
	SetEquippedEmotes = function(_, list)
		CURRENT.values.__equipped = table.clone(list)
	end,
})
add(methods, "RunService", {
	-- Off: the 3D view isn't drawn (a loading screen covers it anyway).
	Set3dRenderingEnabled = function(_, on)
		L.set_3d_rendering(on ~= false)
	end,
	BindToSimulation = function(_, fn)
		return game:GetService("RunService").PreSimulation:Connect(fn)
	end,
})

local RIGID = {
	Weld = true,
	ManualWeld = true,
	Snap = true,
	Glue = true,
	Motor = true,
	Motor6D = true,
	WeldConstraint = true,
	RigidConstraint = true,
}

local function top_of(obj)
	local at = obj
	while at.Parent ~= nil and at.Parent ~= game do
		at = at.Parent
	end
	return at
end

local function ends_of(j)
	local ok, a, b = pcall(function()
		if j:IsA("JointInstance") or j:IsA("WeldConstraint") or j:IsA("NoCollisionConstraint") then
			return j.Part0, j.Part1
		end
		local a0, a1 = j.Attachment0, j.Attachment1
		return a0 and a0.Parent, a1 and a1.Parent
	end)
	if ok then
		return a, b
	end
	return nil, nil
end

local function is_joint(d)
	return d:IsA("JointInstance") or d:IsA("WeldConstraint") or d:IsA("Constraint") or d:IsA("NoCollisionConstraint")
		or d.ClassName == "RigidConstraint"
end

local function joints_of(part)
	local list = {}
	for _, d in ipairs(top_of(part):GetDescendants()) do
		if is_joint(d) then
			local a, b = ends_of(d)
			if a == part or b == part then
				table.insert(list, d)
			end
		end
	end
	return list
end

local function enabled(j)
	local ok, on = pcall(function()
		return j.Enabled
	end)
	return not ok or on ~= false
end

local function assembly(part, recursive)
	local found, list = { [part] = true }, { part }
	local i = 1
	while list[i] do
		local at = list[i]
		for _, j in ipairs(joints_of(at)) do
			if RIGID[j.ClassName] and enabled(j) then
				local a, b = ends_of(j)
				local other = a == at and b or a
				if other and other:IsA("BasePart") and not found[other] then
					found[other] = true
					table.insert(list, other)
				end
			end
		end
		if not recursive then
			break
		end
		i += 1
	end
	return list
end

local function volume(p)
	local s = p.Size
	return s.X * s.Y * s.Z
end

local function root_of(part)
	local best
	for _, p in ipairs(assembly(part, true)) do
		if p.Anchored then
			return p
		end
		if not best then
			best = p
		else
			local a, b = p.RootPriority, best.RootPriority
			local hrp = p.Name == "HumanoidRootPart" and p.Parent and p.Parent:FindFirstChildOfClass("Humanoid")
			if hrp or a > b or (a == b and volume(p) > volume(best)) then
				best = p
			end
		end
	end
	return best or part
end

add(methods, "BasePart", {
	GetRootPart = function(self)
		return root_of(self)
	end,
	IsGrounded = function(self)
		for _, p in ipairs(assembly(self, true)) do
			if p.Anchored then
				return true
			end
		end
		return false
	end,
	GetNoCollisionConstraints = function(self)
		local list = {}
		for _, j in ipairs(joints_of(self)) do
			if j:IsA("NoCollisionConstraint") then
				table.insert(list, j)
			end
		end
		return list
	end,
	CanCollideWith = function(self, other)
		if typeof(other) ~= "Instance" or not other:IsA("BasePart") then
			error("CanCollideWith expects a part", 2)
		end
		if not (self.CanCollide and other.CanCollide) then
			return false
		end
		for _, j in ipairs(joints_of(self)) do
			if j:IsA("NoCollisionConstraint") and enabled(j) then
				local a, b = ends_of(j)
				if a == other or b == other then
					return false
				end
			end
		end
		return game:GetService("PhysicsService"):CollisionGroupsAreCollidable(self.CollisionGroup, other.CollisionGroup)
	end,
	Resize = function(self, face, delta)
		local name = typeof(face) == "EnumItem" and face.Name or tostring(face)
		local normal = Vector3.FromNormalId(name)
		local axis = Vector3.new(math.abs(normal.X), math.abs(normal.Y), math.abs(normal.Z))
		local size = self.Size + axis * delta
		if size.X < 0 or size.Y < 0 or size.Z < 0 then
			return false
		end
		local cf = self.CFrame
		self.Size = size
		self.CFrame = cf * CFrame.new(normal * (delta / 2))
		return true
	end,
})
add(getters, "BasePart", {
	AssemblyRootPart = function(self)
		return root_of(self)
	end,
	CurrentPhysicalProperties = function(self)
		local custom = self.CustomPhysicalProperties
		if custom then
			return custom
		end
		return PhysicalProperties.new(self.Material)
	end,
	ExtentsCFrame = function(self)
		return self.CFrame
	end,
})

add(methods, "Terrain", {
	ReadVoxelChannels = function(self, region, resolution, ids)
		local materials, occupancy = self:ReadVoxels(region, resolution)
		local size = materials.Size
		local out = { Size = size }
		local function each(f)
			local t = {}
			for x = 1, size.X do
				t[x] = {}
				for y = 1, size.Y do
					t[x][y] = {}
					for z = 1, size.Z do
						t[x][y][z] = f(materials[x][y][z], occupancy[x][y][z])
					end
				end
			end
			return t
		end
		for _, id in ipairs(ids or {}) do
			if id == "SolidMaterial" then
				out.SolidMaterial = each(function(m)
					return m == Enum.Material.Water and Enum.Material.Air or m
				end)
			elseif id == "SolidOccupancy" then
				out.SolidOccupancy = each(function(m, o)
					return m == Enum.Material.Water and 0 or o
				end)
			elseif id == "LiquidOccupancy" then
				out.LiquidOccupancy = each(function(m, o)
					return m == Enum.Material.Water and o or 0
				end)
			end
		end
		return out
	end,
	WriteVoxelChannels = function(self, region, resolution, channels)
		local solid = channels.SolidMaterial
		local occupancy = channels.SolidOccupancy
		local liquid = channels.LiquidOccupancy
		local base = solid or liquid
		if not base then
			return
		end
		local materials, amounts = {}, {}
		for x, plane in ipairs(base) do
			materials[x], amounts[x] = {}, {}
			for y, row in ipairs(plane) do
				materials[x][y], amounts[x][y] = {}, {}
				for z in ipairs(row) do
					local m = solid and solid[x][y][z] or Enum.Material.Air
					local o = occupancy and occupancy[x][y][z] or (m ~= Enum.Material.Air and 1 or 0)
					local w = liquid and liquid[x][y][z] or 0
					if (m == Enum.Material.Air or o <= 0) and w > 0 then
						m, o = Enum.Material.Water, w
					end
					materials[x][y][z], amounts[x][y][z] = m, o
				end
			end
		end
		self:WriteVoxels(region, resolution, materials, amounts)
	end,
})

add(methods, "Instance", {
	children = function(self)
		return self:GetChildren()
	end,
	getChildren = function(self)
		return self:GetChildren()
	end,
	clone = function(self)
		return self:Clone()
	end,
	destroy = function(self)
		self:Destroy()
	end,
	Remove = function(self)
		self.Parent = nil
	end,
	remove = function(self)
		self.Parent = nil
	end,
	isA = function(self, name)
		return self:IsA(name)
	end,
	findFirstChild = function(self, name, recursive)
		return self:FindFirstChild(name, recursive)
	end,
	isDescendantOf = function(self, other)
		return self:IsDescendantOf(other)
	end,
	GetActor = get_actor,
})
add(methods, "Model", {
	GetModelSize = function(self)
		return self:GetExtentsSize()
	end,
	move = function(self, at)
		self:MoveTo(at)
	end,
	moveTo = function(self, at)
		self:MoveTo(at)
	end,
})
add(methods, "BasePart", {
	getMass = function(self)
		return self:GetMass()
	end,
})

local function axis_setter(primary)
	return function(self, v)
		local cf = self.CFrame
		local x, y = cf.XVector, cf.YVector
		if primary then
			x = v.Unit
		else
			y = v.Unit
		end
		local z = x:Cross(y)
		if z.Magnitude < 1e-6 then
			return
		end
		z = z.Unit
		y = z:Cross(x).Unit
		self.CFrame = CFrame.fromMatrix(cf.Position, x, y, z)
	end
end
add(getters, "Attachment", {
	Axis = function(self)
		return self.CFrame.XVector
	end,
	SecondaryAxis = function(self)
		return self.CFrame.YVector
	end,
	WorldAxis = function(self)
		local cf = self.WorldCFrame
		return cf and cf.XVector
	end,
	WorldSecondaryAxis = function(self)
		local cf = self.WorldCFrame
		return cf and cf.YVector
	end,
})
add(setters, "Attachment", {
	Axis = axis_setter(true),
	SecondaryAxis = axis_setter(false),
})
add(methods, "Attachment", {
	GetConstraints = function(self)
		local list = {}
		for _, d in ipairs(top_of(self):GetDescendants()) do
			if d:IsA("Constraint") or d.ClassName == "RigidConstraint" then
				local ok, a0, a1 = pcall(function()
					return d.Attachment0, d.Attachment1
				end)
				if ok and (a0 == self or a1 == self) then
					table.insert(list, d)
				end
			end
		end
		return list
	end,
})

add(methods, "Humanoid", {
	PlayEmote = function()
		return false
	end,
})
plain_signals("Animator", { "AnimationPlayed" })
add(methods, "Animator", {
	StepAnimations = function() end,
	ApplyJointVelocities = function() end,
})
add(methods, "Player", {
	IsInGroupAsync = function(self, id)
		return self:IsInGroup(id)
	end,
	GetRankInGroupAsync = function(self, id)
		return self:GetRankInGroup(id)
	end,
	GetRoleInGroupAsync = function(self, id)
		return self:GetRoleInGroup(id)
	end,
})

for _, class in { "DragDetector", "UIDragDetector" } do
	plain_signals(class, { "DragStart", "DragContinue", "DragEnd" })
	add(methods, class, {
		SetDragStyleFunction = NOOP,
		SetPermissionPolicyFunction = NOOP,
		AddConstraintFunction = function()
			return { Disconnect = NOOP, Connected = true }
		end,
		RemoveConstraintFunction = NOOP,
		GetReferenceFrame = function(self)
			local p = self.Parent
			return p and p:IsA("BasePart") and p.CFrame or CFrame.new()
		end,
		RestartDrag = NOOP,
	})
end

add(methods, "MeshPart", {
	ApplyMesh = function(self, from)
		if typeof(from) == "Instance" then
			pcall(function()
				self.MeshId = from.MeshId
			end)
		end
	end,
})
plain_signals("RocketPropulsion", { "ReachedTarget" })
add(methods, "RocketPropulsion", { Fire = NOOP, Abort = NOOP })
add(methods, "WireframeHandleAdornment", {
	AddLine = NOOP,
	AddLines = NOOP,
	AddPath = NOOP,
	AddText = NOOP,
	Clear = NOOP,
})
add(methods, "DataStoreSetOptions", {
	SetMetadata = function(_, t)
		CURRENT.values.__metadata = table.clone(t or {})
	end,
	GetMetadata = function()
		return table.clone(CURRENT.values.__metadata or {})
	end,
})
add(methods, "Workspace", {
	GetPhysicsThrottling = function()
		return 100
	end,
})
add(methods, "HttpService", {
	GetAsync = function()
		error("Http requests are not enabled", 0)
	end,
	PostAsync = function()
		error("Http requests are not enabled", 0)
	end,
	RequestAsync = function()
		error("Http requests are not enabled", 0)
	end,
	GetSecret = function(_, key)
		error("Can't find secret with given key: " .. tostring(key), 2)
	end,
})
add(methods, "PathfindingService", {
	FindPathAsync = function(self, from, to)
		local path = self:CreatePath()
		path:ComputeAsync(from, to)
		return path
	end,
})
SERVICE_EXTRAS.ContextActionService.methods.GetButton = NOOP
SERVICE_EXTRAS.TextService.methods.GetTextBoundsAsync = function(self, params)
	local width = params.Width
	if not width or width <= 0 then
		width = math.huge
	end
	return self:GetTextSize(params.Text or "", params.Size or 14, Enum.Font.SourceSans, Vector2.new(width, math.huge))
end
SERVICE_EXTRAS.PhysicsService = {
	methods = {
		GetMaxCollisionGroups = returns(32),
		RenameCollisionGroup = function(self, from, to)
			if from == "Default" or not self:IsCollisionGroupRegistered(from) then
				error("can't rename collision group " .. tostring(from), 2)
			end
			local groups = self:GetRegisteredCollisionGroups()
			self:RegisterCollisionGroup(to)
			for _, g in ipairs(groups) do
				local name = g.name or g.Name
				local other = name == from and to or name
				self:CollisionGroupSetCollidable(to, other, self:CollisionGroupsAreCollidable(from, name))
			end
			for _, d in ipairs(game:GetDescendants()) do
				if d:IsA("BasePart") and d.CollisionGroup == from then
					d.CollisionGroup = to
				end
			end
			self:UnregisterCollisionGroup(from)
		end,
	},
}

-- Members' older spellings (mostly lower-case) that scripts written long
-- ago still use, and the names they go by now. Only asked once nothing
-- else answers, so a class's own member of that name always wins.
local OLDER_NAMES = {
	-- Every instance
	children = "GetChildren",
	getChildren = "GetChildren",
	clone = "Clone",
	destroy = "Destroy",
	remove = "Remove",
	findFirstChild = "FindFirstChild",
	isA = "IsA",
	isDescendantOf = "IsDescendantOf",
	childAdded = "ChildAdded",
	changed = "Changed",
	-- The game and its services
	service = "GetService",
	getService = "GetService",
	getPlayers = "GetPlayers",
	players = "GetPlayers",
	playerFromCharacter = "GetPlayerFromCharacter",
	getMinutesAfterMidnight = "GetMinutesAfterMidnight",
	setMinutesAfterMidnight = "SetMinutesAfterMidnight",
	findPartOnRay = "FindPartOnRay",
	findPartsInRegion3 = "FindPartsInRegion3",
	-- Parts, models and characters
	breakJoints = "BreakJoints",
	makeJoints = "MakeJoints",
	resize = "Resize",
	getMass = "GetMass",
	takeDamage = "TakeDamage",
	loadAnimation = "LoadAnimation",
	lastForce = "GetLastForce",
	-- Sounds, tools and click detectors
	play = "Play",
	stop = "Stop",
	pause = "Pause",
	equipped = "Equipped",
	unequipped = "Unequipped",
	mouseClick = "MouseClick",
	keyDown = "KeyDown",
}

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
	-- An older spelling of a member: answer with today's.
	local newer = OLDER_NAMES[key]
	if newer then
		local ok, v = pcall(function()
			return obj[newer]
		end)
		if ok and v ~= nil then
			return true, v
		end
	end
	return false
end

-- `allowed`: the class database knows `key`, so it may be stored even
-- when the layer has nothing for it.
function L.new_index(key_id, obj, key, value, groups, allowed)
	local s = slot(key_id)
	for _, group in ipairs(groups) do
		local set = setters[group] and setters[group][key]
		if set then
			set(obj, value, s)
			L.fire_changed(key_id, key)
			return true
		end
	end
	local known = s.values[key] ~= nil or allowed
	if not known then
		for _, group in ipairs(groups) do
			if (props[group] and props[group][key] ~= nil) or (getters[group] and getters[group][key]) then
				known = true
				break
			end
		end
	end
	if known then
		local before = s.values[key]
		s.values[key] = value == nil and NIL or value
		if before ~= s.values[key] then
			L.fire_changed(key_id, key)
		end
		return true
	end
	return false
end

-- Whether the layer handles setting `key` itself for these groups.
function L.has_setter(groups, key)
	for _, group in ipairs(groups) do
		if setters[group] and setters[group][key] then
			return true
		end
	end
	return false
end

function L.signal()
	return new_signal()
end

-- Fires an object's event `key` with where the mouse is, if a script made it.
function L.fire_mouse(key_id, key)
	local s = store[key_id]
	local sig = s and s.signals[key]
	if not sig then
		return
	end
	local player = game:GetService("Players").LocalPlayer
	local x, y = 0, 0
	if player then
		local m = player:GetMouse()
		x, y = m.X, m.Y
	end
	sig:Fire(x, y)
end

function L.fire_event(key_id, key, ...)
	local s = store[key_id]
	local sig = s and s.signals[key]
	if sig then
		sig:Fire(...)
	end
end

-- The event for changes to member `key` of an object, made when first asked.
function L.changed_signal(key_id, key)
	local s = slot(key_id)
	s.changed = s.changed or {}
	local sig = s.changed[key]
	if not sig then
		sig = new_signal()
		s.changed[key] = sig
	end
	return sig
end

function L.fire_changed(key_id, key)
	local s = store[key_id]
	local sig = s and s.changed and s.changed[key]
	if sig then
		sig:Fire()
	end
	local any = s and s.signals.Changed
	if any then
		any:Fire(key)
	end
end

-- Members every instance has, for objects that only stand in for one
-- (services and settings libroxel makes in Lua): (found, value).
local COMMON = {
	GetPropertyChangedSignal = function(key_id, _, _, key)
		return L.changed_signal(key_id, key)
	end,
	GetAttribute = function(key_id, _, _, key)
		local a = slot(key_id).attributes
		return a and a[key]
	end,
	SetAttribute = function(key_id, _, _, key, value)
		local s = slot(key_id)
		s.attributes = s.attributes or {}
		s.attributes[key] = value
		L.fire_changed(key_id, "@" .. key)
		local sig = s.signals.AttributeChanged
		if sig then
			sig:Fire(key)
		end
	end,
	GetAttributes = function(key_id)
		return table.clone(slot(key_id).attributes or {})
	end,
	GetAttributeChangedSignal = function(key_id, _, _, key)
		return L.changed_signal(key_id, "@" .. key)
	end,
	IsA = function(_, name, _, other)
		return other == name or other == "Instance"
	end,
	GetFullName = function(_, name)
		return name
	end,
	FindFirstChild = function() end,
	FindFirstChildOfClass = function() end,
	FindFirstChildWhichIsA = function() end,
	FindFirstAncestor = function() end,
	FindFirstAncestorOfClass = function() end,
	FindFirstAncestorWhichIsA = function() end,
	FindFirstDescendant = function() end,
	WaitForChild = function() end,
	GetChildren = function()
		return {}
	end,
	GetDescendants = function()
		return {}
	end,
	IsDescendantOf = function(_, _, _, other)
		return other == game
	end,
	IsAncestorOf = function()
		return false
	end,
	Destroy = function() end,
	ClearAllChildren = function() end,
	GetTags = function()
		return {}
	end,
	HasTag = function()
		return false
	end,
	AddTag = function() end,
	RemoveTag = function() end,
}
local COMMON_SIGNALS =
	{ "Changed", "AttributeChanged", "ChildAdded", "ChildRemoved", "DescendantAdded", "DescendantRemoving", "AncestryChanged", "Destroying" }

function L.common(key_id, name, key)
	local m = COMMON[key]
	if m then
		return true, function(_, ...)
			return m(key_id, name, nil, ...)
		end
	end
	if table.find(COMMON_SIGNALS, key) then
		local s = slot(key_id)
		local sig = s.signals[key]
		if not sig then
			sig = new_signal()
			s.signals[key] = sig
		end
		return true, sig
	end
	if key == "Parent" then
		return true, game
	elseif key == "Archivable" then
		return true, true
	elseif key == "ClassName" or key == "Name" then
		return true, name
	end
	return false
end

function L.forget(keys)
	for _, key_id in ipairs(keys) do
		store[key_id] = nil
	end
end

------------------------------------------------------------------------------
-- Globals
------------------------------------------------------------------------------

function L.globals()
	State = Enum.HumanoidStateType
	-- Parallel Luau: libroxel runs every script on one thread, so these
	-- have nothing to switch.
	g.task.synchronize = function() end
	g.task.desynchronize = function() end
	-- Profiler labels and memory categories: nothing to record them in.
	local dbg = g.debug
	dbg.profilebegin = function() end
	dbg.profileend = function() end
	dbg.setmemorycategory = function() end
	dbg.resetmemorycategory = function() end
	dbg.dumpcodesize = function() end
	local NORMALS = {
		Top = Vector3.new(0, 1, 0),
		Bottom = Vector3.new(0, -1, 0),
		Right = Vector3.new(1, 0, 0),
		Left = Vector3.new(-1, 0, 0),
		Back = Vector3.new(0, 0, 1),
		Front = Vector3.new(0, 0, -1),
	}
	g.Vector3.FromNormalId = function(id)
		return NORMALS[typeof(id) == "string" and id or id.Name] or Vector3.zero
	end
	g.Color3.toHSV = function(c)
		return c:ToHSV()
	end
	g.ElapsedTime = function()
		return elapsedTime()
	end
	g.PathWaypoint = table.freeze({
		new = function(position, action, label)
			return table.freeze({
				Position = position or Vector3.zero,
				Action = action or Enum.PathWaypointAction.Walk,
				Label = label or "",
			})
		end,
	})
	g.Vector3.FromAxis = function(axis)
		local n = typeof(axis) == "string" and axis or axis.Name
		return n == "X" and Vector3.xAxis or n == "Y" and Vector3.yAxis or Vector3.zAxis
	end
	-- The newer lower-case spellings of the two above.
	g.Vector3.fromNormalId = g.Vector3.FromNormalId
	g.Vector3.fromAxis = g.Vector3.FromAxis
	-- A new object like `existing`, with its properties but none of its children.
	g.Instance.fromExisting = function(existing)
		if typeof(existing) ~= "Instance" then
			error("Instance.fromExisting expects an Instance", 2)
		end
		local archivable = existing.Archivable
		existing.Archivable = true
		local copy = existing:Clone()
		existing.Archivable = archivable
		copy:ClearAllChildren()
		copy.Archivable = archivable
		return copy
	end
	-- Content: a picture by address, or one a script draws (an EditableImage).
	local function content(kind, uri, object)
		return table.freeze({ SourceType = kind, Uri = uri, Object = object })
	end
	g.Content = table.freeze({
		none = content("None", nil, nil),
		fromUri = function(uri)
			return content("Uri", tostring(uri), nil)
		end,
		fromAssetId = function(id)
			return content("Uri", "rbxassetid://" .. tostring(id), nil)
		end,
		fromObject = function(obj)
			if obj == nil then
				error("Content.fromObject expects an EditableImage or EditableMesh", 2)
			end
			return content("Object", nil, obj)
		end,
	})
	-- Tables shared between Actors: with one thread, a plain table is enough.
	g.SharedTable = table.freeze({
		new = function(t)
			return t and table.clone(t) or {}
		end,
		clone = function(t, deep)
			return table.clone(t)
		end,
		cloneAndFreeze = function(t)
			return table.freeze(table.clone(t))
		end,
		isFrozen = table.isfrozen,
		size = function(t)
			local n = 0
			for _ in pairs(t) do
				n += 1
			end
			return n
		end,
		update = function(t, key, f)
			t[key] = f(t[key])
		end,
		increment = function(t, key, by)
			local old = t[key] or 0
			t[key] = old + by
			return old
		end,
	})
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
		return { CFrame = CFrame.new((min + max) / 2), Size = max - min, Min = min, Max = max }
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

	local function section(name, props)
		return stand_in(name, { props = props }, "settings:" .. name)
	end
	local settings_tree = {
		Rendering = section(
			"RenderSettings",
			{ QualityLevel = "QualityLevel.Automatic", MeshPartDetailLevel = "MeshPartDetailLevel.DistanceBased", EditQualityLevel = "QualityLevel.Automatic" }
		),
		Physics = section("PhysicsSettings", { AllowSleep = true, PhysicsEnvironmentalThrottle = NIL, AreOwnersShown = false }),
		Network = section("NetworkSettings", { IncomingReplicationLag = 0 }),
		Diagnostics = section("DebugSettings", {}),
	}
	g.settings = function()
		return settings_tree
	end
	local user_game_settings = stand_in("UserGameSettings", {
		props = {
			MasterVolume = 1,
			SavedQualityLevel = "SavedQualitySetting.Automatic",
			ComputerMovementMode = "ComputerMovementMode.UserChoice",
			ComputerCameraMovementMode = "ComputerCameraMovementMode.UserChoice",
			TouchMovementMode = "TouchMovementMode.UserChoice",
			MouseSensitivity = 1,
			RotationType = "RotationType.MovementRelative",
			GamepadCameraSensitivity = 1,
			Fullscreen = false,
			GraphicsQualityLevel = 0,
		},
		signals = { "FullscreenChanged", "StudioModeChanged" },
		methods = {
			GetCameraYInvertValue = function()
				return 1
			end,
			SetCameraYInvertVisible = function() end,
			InFullScreen = function()
				return false
			end,
			InStudioMode = function()
				return false
			end,
		},
	}, "settings:UserGameSettings")
	local user_settings = stand_in("UserSettings", {
		props = { GameSettings = user_game_settings },
		methods = {
			GetService = function(_, name)
				if name == "UserGameSettings" then
					return user_game_settings
				end
				error(tostring(name) .. " is not a valid service name", 0)
			end,
			IsUserFeatureEnabled = function()
				return false
			end,
		},
	}, "settings:UserSettings")
	g.UserSettings = function()
		return user_settings
	end
end

-- Touches ---------------------------------------------------------------
-- The app passes every finger on the screen here; scripts hear of them as
-- on the reference platform: TouchStarted, TouchMoved and TouchEnded with one InputObject
-- per finger (the same one from start to end, as the reference platform gives), InputBegan,
-- InputChanged and InputEnded with it too, and the gestures made of them:
-- TouchTap, TouchTapInWorld, TouchLongPress, TouchPan, TouchPinch,
-- TouchRotate and TouchSwipe.

local UIS = "service:UserInputService"
-- How far a finger may wander and still be tapping (pixels), how long a
-- long press is, and how fast and far a swipe goes.
local SLOP, LONG_PRESS, SWIPE_TIME, SWIPE_DISTANCE = 10, 0.5, 0.4, 50

local fingers = {} -- id -> { obj, start, at, began, moved, gui }
local order = {} -- ids, first finger first
local gesture = { most = 0 } -- what the fingers on the screen are doing

local function uis_event(name, ...)
	L.fire_event(UIS, name, ...)
end

local function positions()
	local out = {}
	for _, id in order do
		local f = fingers[id]
		table.insert(out, Vector2.new(f.at.X, f.at.Y))
	end
	return out
end

local function two()
	local a, b = fingers[order[1]], fingers[order[2]]
	if a and b and not order[3] then
		return a.at, b.at
	end
	return nil
end

-- Ends whichever gestures are going on, as the reference platform does when a finger lifts.
local function end_two(processed)
	if gesture.pinch then
		uis_event("TouchPinch", positions(), gesture.scale or 1, 0, Enum.UserInputState.End, processed)
		gesture.pinch = nil
	end
	if gesture.rotate then
		uis_event("TouchRotate", positions(), gesture.rotation or 0, 0, Enum.UserInputState.End, processed)
		gesture.rotate = nil
	end
end

function L.touch_event(id, phase, x, y, processed, t)
	local at = Vector2.new(x, y)
	local f = fingers[id]
	if phase == "began" then
		local obj = {
			UserInputType = Enum.UserInputType.Touch,
			UserInputState = Enum.UserInputState.Begin,
			KeyCode = Enum.KeyCode.Unknown,
			Position = Vector3.new(x, y, 0),
			Delta = Vector3.zero,
		}
		function obj:IsA(class)
			return class == "InputObject" or class == "Instance"
		end
		function obj:IsModifierKeyDown()
			return false
		end
		end_two(processed)
		f = { obj = obj, start = at, at = at, began = t, last = t, moved = false, gui = processed }
		fingers[id] = f
		table.insert(order, id)
		gesture.most = math.max(gesture.most, #order)
		if #order == 2 then
			local a, b = two()
			gesture.width = (a - b).Magnitude
			gesture.angle = math.deg(math.atan2(b.Y - a.Y, b.X - a.X))
		end
		L.fire_input("began", obj, processed)
		uis_event("TouchStarted", obj, processed)
		return
	end
	if not f then
		return
	end
	local obj = f.obj
	if phase == "moved" then
		local dt = math.max(t - f.last, 1 / 240)
		local moved = at - f.at
		f.at, f.last = at, t
		obj.Delta = Vector3.new(moved.X, moved.Y, 0)
		obj.Position = Vector3.new(x, y, 0)
		obj.UserInputState = Enum.UserInputState.Change
		uis_event("InputChanged", obj, processed)
		uis_event("TouchMoved", obj, processed)
		if not f.moved and (at - f.start).Magnitude > SLOP then
			f.moved = true
			if f.pressed then
				uis_event("TouchLongPress", positions(), Enum.UserInputState.End, processed)
				f.pressed = nil
			end
		end
		if f.moved and id == order[1] then
			local state = gesture.pan and Enum.UserInputState.Change or Enum.UserInputState.Begin
			gesture.pan = true
			uis_event("TouchPan", positions(), at - f.start, moved / dt, state, processed)
		end
		local a, b = two()
		if a and gesture.width and gesture.width > 0 then
			local scale = (a - b).Magnitude / gesture.width
			local state = gesture.pinch and Enum.UserInputState.Change or Enum.UserInputState.Begin
			local speed = (scale - (gesture.scale or 1)) / dt
			gesture.pinch, gesture.scale = true, scale
			uis_event("TouchPinch", positions(), scale, speed, state, processed)
			local angle = math.deg(math.atan2(b.Y - a.Y, b.X - a.X))
			local rotation = (angle - gesture.angle + 180) % 360 - 180
			state = gesture.rotate and Enum.UserInputState.Change or Enum.UserInputState.Begin
			speed = (rotation - (gesture.rotation or 0)) / dt
			gesture.rotate, gesture.rotation = true, rotation
			uis_event("TouchRotate", positions(), rotation, speed, state, processed)
		end
		return
	end
	-- The finger lifted.
	obj.Position = Vector3.new(x, y, 0)
	obj.Delta = Vector3.zero
	obj.UserInputState = Enum.UserInputState.End
	f.at = at
	local held = t - f.began
	local travelled = at - f.start
	if f.pressed then
		uis_event("TouchLongPress", positions(), Enum.UserInputState.End, processed)
		L.gui_touch(x, y, "TouchLongPress", positions(), Enum.UserInputState.End)
	elseif not f.moved and #order == 1 then
		uis_event("TouchTap", { at }, processed)
		L.gui_touch(x, y, "TouchTap", { at })
		uis_event("TouchTapInWorld", at, processed)
	elseif f.moved and held <= SWIPE_TIME and travelled.Magnitude >= SWIPE_DISTANCE then
		local direction
		if math.abs(travelled.X) > math.abs(travelled.Y) then
			direction = travelled.X > 0 and Enum.SwipeDirection.Right or Enum.SwipeDirection.Left
		else
			direction = travelled.Y > 0 and Enum.SwipeDirection.Down or Enum.SwipeDirection.Up
		end
		uis_event("TouchSwipe", direction, gesture.most, processed)
	end
	end_two(processed)
	if id == order[1] and gesture.pan then
		uis_event("TouchPan", positions(), travelled, Vector2.zero, Enum.UserInputState.End, processed)
		gesture.pan = nil
	end
	L.fire_input("ended", obj, processed)
	uis_event("TouchEnded", obj, processed)
	fingers[id] = nil
	table.remove(order, table.find(order, id))
	if #order == 0 then
		gesture = { most = 0 }
	elseif #order == 2 then
		local a, b = two()
		gesture.width = (a - b).Magnitude
		gesture.angle = math.deg(math.atan2(b.Y - a.Y, b.X - a.X))
	end
end

-- A finger held still for a while is a long press.
function L.touch_tick(t)
	if #order ~= 1 then
		return
	end
	local f = fingers[order[1]]
	if not f.moved and not f.pressed and t - f.began >= LONG_PRESS then
		f.pressed = true
		uis_event("TouchLongPress", positions(), Enum.UserInputState.Begin, f.gui)
		L.gui_touch(f.at.X, f.at.Y, "TouchLongPress", positions(), Enum.UserInputState.Begin)
	end
end

-- Whether the game wants the stick and jump button on screen: not when a
-- script set UserInputService.ModalEnabled, turned GuiService's
-- TouchControlsEnabled off, or moves the character itself
-- (DevTouchMovementMode Scriptable).
function L.touch_controls_shown()
	local uis = store[UIS]
	if uis and uis.values.ModalEnabled == true then
		return false
	end
	local gui = store["service:GuiService"]
	if gui and gui.values.TouchControlsEnabled == false then
		return false
	end
	local ok, mode = pcall(function()
		return game:GetService("Players").LocalPlayer.DevTouchMovementMode
	end)
	if ok and typeof(mode) == "EnumItem" and mode.Name == "Scriptable" then
		return false
	end
	return true
end

return L
