# Libroxel

**Build voxel worlds together, and script them in Luau.**

Libroxel is an open-source, multiplayer game-creation platform. Worlds are
built from blocks and parts, behaviour is written in [Luau](https://luau.org),
and the engine runs on [Bevy](https://bevy.org).

> **Status: early.** There is an editor where you can build a world, write
> scripts and press Play to test it with a character, or host it and have
> friends join from their own computers. Nothing is stable yet.

![The Libroxel editor](docs/editor.png)

![Playing the sample obby: the leaderboard, the coin counter and a "hold E" prompt](docs/playing.png)

## Getting started

You need a recent stable Rust toolchain (`rustup update stable`).

On Arch Linux, Bevy's build needs a few system packages:

```sh
sudo pacman -S --needed base-devel pkgconf wayland libxkbcommon systemd-libs vulkan-icd-loader
# plus the Vulkan driver for your GPU, for example vulkan-radeon, vulkan-intel or nvidia-utils
```

On Debian or Ubuntu the equivalent is
`sudo apt install build-essential pkg-config libwayland-dev libxkbcommon-dev libudev-dev`.

Then:

```sh
cargo run -p libroxel-editor                      # the editor
cargo run -p libroxel-editor -- --sample          # the editor, with the sample obby open
cargo run -p libroxel-proto -- my_game.lrx        # play a saved game without the editor
cargo test --workspace                            # every test, no GPU needed
```

The first build compiles Bevy and takes a while. After that it is quick.

## The editor

- **File → Open sample obby** loads a small obstacle course with a wooden
  ramp, spinning coins, lava, fading platforms, a lift and a finish pad, plus
  a leaderboard that counts coins and deaths, an on-screen coin counter, a
  day/night button, a vault whose door opens when you click it, and a stand
  where you hold E to buy a speed boost. Coins are saved between play tests,
  and a builder tool (press 1, then click) places bricks. There's also a
  launch pad, street lamps that light up at night, TNT that blows up a stack
  of crates, a chat box (press /) that floats what you say over your head,
  an overhead camera (press C), and a zombie in the quarry that chases you.
  Everyone starts on the Runners team and joins the Winners at the finish.
  A health bar sits in the corner, the quarry has a mine cart (its sides are
  welded on) that you can click to push, and a pressure plate by the shop
  glows while you stand on it. Past the quarry is a walled kart track with a
  go-kart to drive (walk into its seat, WASD to drive, Space to hop out), and
  there's a bench to sit on at the start. Over the track's right wall is a
  physics playground: a tyre swing on a rope, a hovering pad held up by an
  AlignPosition, a weight bouncing on a spring, a windmill turned by a
  hinge motor and a gate on a servo hinge (click it). The quarry has a smoking
  campfire, the launch pad fizzes, the finish sparkles, and the zombie
  glows red so you can spot it.
  Press **▶ Play** (or F5) to try it.
- **Explorer** (left) shows the instance tree. Click to select, double-click
  a script to edit it, drag one item onto another to move it inside, and
  right-click for Insert, Rename, Duplicate and Delete. **Insert → Character
  (NPC)** drops in a dummy with a script that walks it back and forth, and
  **Insert → Go-kart** drops in a ready-to-drive kart.
- **Properties** (right) edits the selection: name, position, orientation,
  size, shape, colour, material, anchored, can-collide, transparency, and so on.
- **Parts** come in four shapes (block, ball, cylinder, wedge) and eight
  materials (plastic, smooth plastic, neon, metal, glass, wood, brick, grass).
  Insert them from **+ Part** or the Insert menu.
- **Viewport:** hold the right mouse button and use WASD/QE to fly (Shift is
  faster), scroll to move forward. Click a part to select it, then drag the
  handles to move, resize or rotate it (Ctrl+1 to Ctrl+4 switch tools, and
  Ctrl+R turns the selection 90°). The two snap boxes set the grid step and
  the rotation step.
- **Play / Stop:** Play starts every Script, then you join as `Player1` and
  your character spawns at the first SpawnLocation. If scripts give you a
  `leaderstats` folder, the leaderboard shows it in the top-right corner. Walk with WASD, jump with Space, turn the camera with
  the right mouse button. Stop (or Esc) puts the world back as it was.
- **Screen GUIs:** put a `ScreenGui` in StarterGui and fill it with frames,
  text labels and text buttons (Insert menu). They are drawn over the view
  while you edit, so you can click one to select it and lay it out with
  Position, Size and AnchorPoint. While playing, every player gets a copy,
  buttons can be clicked, and parts with a `ClickDetector` can be clicked too.
- **Tools:** put a `Tool` in StarterPack (Insert menu gives it a `Handle` and
  a script). While playing, tools show on a hotbar at the bottom; press 1 to
  9 or click a slot to hold one, and click in the world to use it.
- **Floating text:** a `BillboardGui` inside a part holds labels that float
  above it and face the camera, like signs and name tags.
- **Lights:** a `PointLight` or `SpotLight` inside a part lights the world
  around it; they show best at night.
- **Text boxes and layouts:** a `TextBox` can be typed into while playing.
  A `UICorner` inside a GUI object rounds its corners, and a `UIListLayout`
  inside a frame stacks its other children in a column or row. A
  `ScrollingFrame` scrolls with the mouse wheel, and `UIGridLayout`,
  `UIPadding`, `UIStroke`, `UIScale` and `UIAspectRatioConstraint` can be
  inserted into the selected GUI object.
- **Attributes and tags:** the Properties panel lists the selection's
  attributes (numbers, text, booleans, vectors, colours) and tags, which you
  can add, change and remove. Both are saved with the game.
- **Saved data:** what a game's data stores save during a play test is kept
  beside the game's file (as `<name>.data.json`) and loaded next time you
  press Play. **File → Clear saved data** forgets it.
- **Output** (bottom) shows `print`, `warn` and script errors. While playing
  you can type Luau into its command bar and press Enter.
- **Playing together:** **🌐 Host** plays the game and lets other computers
  join on port 7420 (open it in your firewall to play across a network).
  On another computer, **🔗 Join** asks for the host's address (and a port,
  if not 7420) and a name. The host runs every `Script` and all the physics;
  each joined computer runs its own player's `LocalScript`s, moves its own
  character straight away (telling the host where it went), and shows the
  rest of the world as the host sends it. The toolbar shows how many have joined. To host
  without a window, run
  `cargo run --release -p libroxel-engine --example server -- mygame.lrx`.
- **Undo/redo** with Ctrl+Z and Ctrl+Y. **Save** with Ctrl+S. Games are
  saved as `.lrx` files in `~/Projects/libroxel-games/` by default.

## Layout

| Crate | What it is |
| --- | --- |
| [`libroxel-instance`](crates/libroxel-instance) | The instance tree: classes, typed properties, parenting, destroy, a change journal, and the `.lrx` file format. No engine dependencies. |
| [`libroxel-script`](crates/libroxel-script) | The sandboxed Luau runtime (via `mlua`) with a thread scheduler. Every name scripts can see lives in [`names.rs`](crates/libroxel-script/src/names.rs). |
| [`libroxel-engine`](crates/libroxel-engine) | Play sessions: box physics, the player character, touch events, respawning, and the Bevy integration (rendering and cameras). |
| [`libroxel-editor`](crates/libroxel-editor) | The editor. |
| [`libroxel-proto`](crates/libroxel-proto) | A minimal player that runs a saved game or a single script. |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit together.

## Scripting API

Scripts use a familiar scene-graph style: an `Instance` tree rooted at
`game`, a `workspace`, value types like `Vector3` and `Color3`, services from
`game:GetService`, and signals with `:Connect`. What exists today:

- **Globals:** `game`, `workspace`, `script`, `Instance.new(class, parent?)`,
  `Vector3.new/zero/one`, `Vector2`, `Color3.new/fromRGB`, `CFrame`, `UDim2`,
  `TweenInfo`, `RaycastParams`, `Enum`, `require`, `print`, `warn`, `time()`, `task.wait`,
  `task.spawn`, `task.delay`, and `wait`.
- **Classes:** `Part`, `SpawnLocation`, `Model`, `Folder`, `Script`,
  `LocalScript`, `ModuleScript`, `Humanoid`, `IntValue`, `NumberValue`,
  `StringValue`, `BoolValue`, `ClickDetector`, `ProximityPrompt`,
  `RemoteEvent`, `BindableEvent`, `RemoteFunction`, `BindableFunction`, `ObjectValue`, `Vector3Value`, `Color3Value`, `ScreenGui`, `Frame`, `TextLabel`,
  `TextButton`, `TextBox`, `ScrollingFrame`, `UICorner`, `UIListLayout`,
  `UIGridLayout`, `UIPadding`, `UIStroke`, `UIScale`,
  `UIAspectRatioConstraint`, `BillboardGui`,
  `Tool`, `PointLight`, `SpotLight`, `Explosion`, `Camera`, plus the built-in `Workspace`,
  `Players` (with one `Player` each, holding a `PlayerGui` and a
  `Backpack`), `ReplicatedStorage`, `ServerStorage`, `StarterPlayer`, `ServerScriptService`, `StarterGui`,
  `StarterPack` and `Lighting`.
- **Instance members:** `Name`, `ClassName`, `Parent`, `FindFirstChild`,
  `FindFirstChildOfClass`, `FindFirstAncestor`, `FindFirstDescendant`,
  `WaitForChild`, `GetChildren`, `GetDescendants`, `ClearAllChildren`, `IsA`,
  `IsDescendantOf`, `GetFullName`, `Clone`, `Destroy`, `instance.ChildName`,
  `GetPropertyChangedSignal(name)`, and the `Changed`, `ChildAdded`,
  `ChildRemoved`, `DescendantAdded`, `DescendantRemoving` and `Destroying`
  events. Parts also have `Touched` and `TouchEnded`.
- **Attributes:** `SetAttribute(name, value)` (nil removes it),
  `GetAttribute`, `GetAttributes`, `GetAttributeChangedSignal(name)` and the
  `AttributeChanged(name)` event. Values can be booleans, numbers, strings,
  `Vector3`, `Vector2`, `Color3` and `UDim2`.
- **Tags:** `AddTag`, `RemoveTag`, `HasTag` and `GetTags` on any instance,
  and `CollectionService:GetTagged(tag)`, `GetInstanceAddedSignal(tag)` and
  `GetInstanceRemovedSignal(tag)`.
- **Tools:** tools in StarterPack are copied into each player's `Backpack`
  when their character spawns. Holding one moves it into the character and
  puts its `Handle` part in their hand. A tool fires `Equipped(mouse)`,
  `Unequipped`, and `Activated` / `Deactivated` on click while `Enabled`.
  `Humanoid:EquipTool(tool)` and `UnequipTools()` do it from a script.
- **Mouse:** `Player:GetMouse()` gives `Hit` (a CFrame where the pointer
  meets the world), `Target` (the part under it), `X`, `Y`, and the
  `Button1Down` / `Button1Up` events.
- **Camera:** `workspace.CurrentCamera` has `CFrame`, `FieldOfView`,
  `CameraType` (`Custom` follows the character, `Scriptable` stays where
  scripts put it, and tweens work on it), `ViewportSize` and
  `WorldToViewportPoint(point)`.
- **Moving parts:** `AssemblyLinearVelocity` reads or sets how fast a part
  (or a character's root part) is moving, `ApplyImpulse(vector)` pushes it,
  and `GetMass()` says how heavy it is.
- **Explosions:** an `Explosion` (`Position`, `BlastRadius`,
  `BlastPressure`) goes off when it is put in the workspace. Its `Hit` event
  fires with each part in reach and the distance; characters in reach die
  and loose parts are thrown.
- **Text boxes:** `TextBox` has `Text`, `PlaceholderText` and
  `ClearTextOnFocus`, the `Focused` and `FocusLost(enterPressed)` events, and
  `CaptureFocus`, `ReleaseFocus` and `IsFocused`.
- **Layout helpers:** `UICorner.CornerRadius` and `UIListLayout.Padding` are
  `UDim`s (`UDim.new(scale, offset)`); `UIListLayout.FillDirection` is
  `Vertical` or `Horizontal`, and items go in `LayoutOrder` order.
  `UIGridLayout` has `CellSize`, `CellPadding` (`UDim2`s) and
  `FillDirection`. `UIPadding` has `PaddingLeft`, `PaddingRight`,
  `PaddingTop` and `PaddingBottom`. `UIStroke` outlines its parent (or its
  text, for a text object) with `Color`, `Thickness`, `Transparency` and
  `Enabled`. `UIScale.Scale` grows its parent and `UIAspectRatioConstraint`
  keeps its `AspectRatio` (width over height).
- **Images:** `ImageLabel` and `ImageButton` (which has `MouseButton1Click`
  and `Activated` like a text button) show the PNG named by `Image`. A plain
  name is looked for beside the game file, in an `images` folder beside it,
  and in `~/Projects/libroxel-games/images`. Built in are `libroxel://coin`,
  `smile`, `star`, `arrow` and `checker`.
  `ImageColor3` tints it, `ImageTransparency` fades it, and `ScaleType` is
  `Stretch`, `Fit` or `Crop`. Joined computers need the same files.
- **Pictures on parts:** a `Decal` inside a part stretches the PNG named by
  `Texture` over one `Face` (`Enum.NormalId`: Front, Back, Left, Right,
  Top, Bottom), and a `Texture` repeats it every `StudsPerTileU` by
  `StudsPerTileV` studs. `Color3` tints it and `Transparency` fades it.
  Names are found like an `ImageLabel`'s. The sample has an arrow sign, a
  star on the vault door and a checkered finish.
- **Scrolling frames:** `ScrollingFrame` has `CanvasSize` (`UDim2`),
  `CanvasPosition` (`Vector2`, changed by the wheel) and
  `ScrollBarThickness`; its children are placed on the canvas and clipped.
- **Lights:** `PointLight` and `SpotLight` have `Enabled`, `Brightness`,
  `Color` and `Range`; spot lights also have `Angle`.
- **Floating text:** `BillboardGui` has `Size` (`UDim2`, scale in studs plus
  pixels), `StudsOffset`, `AlwaysOnTop`, `MaxDistance` and `Enabled`, and
  shows the labels inside it over its parent part (or a model's `Head`).
- **Helpers:** `tick()`, `elapsedTime()`, `spawn`, `delay` and
  `task.defer(fn, ...)`; `Color3.fromHSV`, `Color3.fromHex`, `ToHSV`,
  `ToHex` and `Lerp`; `Vector3.xAxis`/`yAxis`/`zAxis`, `Min`, `Max`, `Abs`,
  `Floor`, `Ceil`, `Sign`, `FuzzyEq` and `Angle`; `FindFirstChildWhichIsA`,
  `FindFirstAncestorOfClass`, `FindFirstAncestorWhichIsA`, `IsAncestorOf`,
  `Players:GetPlayerByUserId`, and `game.PlaceId` / `game.JobId`.
- **Signals:** `Connect` (returns a connection with `Disconnect`), `Once`,
  and `Wait`, which pauses the script until the signal fires and returns its
  arguments.
- **CFrame:** `CFrame.new`, `CFrame.lookAt`, `CFrame.Angles`,
  `fromOrientation` and `fromAxisAngle`; `Position`, `LookVector`,
  `RightVector`, `UpVector`, `Inverse`, `Lerp`, `ToWorldSpace`,
  `ToObjectSpace`, `PointToWorldSpace`, `PointToObjectSpace`, `*` and `+`.
  Parts have a `CFrame`, and models have `GetPivot` and `PivotTo`.
- **TweenService:** `TweenService:Create(instance, TweenInfo.new(time, style,
  direction, repeats, reverses, delay), goals)` animates numbers, vectors,
  colours, `UDim2`s and `CFrame`s with eleven easing styles. Tweens have
  `Play`, `Pause`, `Cancel` and a `Completed` event.
- **GUIs:** `ScreenGui` (`Enabled`, `ResetOnSpawn`), and `Frame`,
  `TextLabel` and `TextButton` with `Position` and `Size` (`UDim2`),
  `AnchorPoint`, `BackgroundColor3`, `BackgroundTransparency`, `Visible`,
  `Text`, `TextColor3` and `TextSize`, plus `ZIndex` (higher draws on
  top), `BorderSizePixel`, `BorderColor3` and `ClipsDescendants`. Text
  objects also have `TextTransparency`, `TextXAlignment` (`Left`, `Center`,
  `Right`), `TextYAlignment` (`Top`, `Center`, `Bottom`), `TextWrapped` and
  `TextScaled`. Buttons fire `MouseButton1Click` and `Activated`, and every
  GUI object fires `MouseEnter` and `MouseLeave`. `LocalScript`s inside a GUI run for the player, and
  `Players.LocalPlayer` is that player.
- **Clicking parts:** a `ClickDetector` inside a part fires
  `MouseClick(player)` when the player clicks the part from within
  `MaxActivationDistance` studs.
- **Proximity prompts:** a `ProximityPrompt` inside a part shows a "press E"
  bubble when the player is within `MaxActivationDistance`. It has
  `ActionText`, `ObjectText`, `KeyboardKeyCode`, `HoldDuration` and
  `Enabled`, and fires `Triggered(player)` and `TriggerEnded(player)`.
- **Lighting:** `ClockTime` (0 to 24) moves the sun and colours the sky,
  `Brightness` sets how strong the sun is, `Ambient` tints the shadows (0.5
  grey is normal), and things further than `FogStart` studs fade into
  `FogColor` until they vanish at `FogEnd`.
- **Movers and constraints:** an `Attachment` is a point on a part
  (`Position` relative to the part, `WorldPosition` to read). With
  `Attachment0` set, `LinearVelocity` (`VectorVelocity`, `MaxForce`) pushes
  the part towards a velocity, `VectorForce` (`Force`) pushes it steadily,
  and `AlignPosition` pulls it to `Attachment1` or to `Position`
  (`Responsiveness`, `MaxForce`, `MaxVelocity`, `RigidityEnabled`).
  `RopeConstraint` (`Length`) and `SpringConstraint` (`FreeLength`,
  `Stiffness`, `Damping`) join two attachments, and are drawn when
  `Visible` (`Color`, `Thickness`). The older `BodyVelocity`, `BodyPosition`
  (`P`, `D`) and `BodyForce` work inside a part. Movers push whole welded
  assemblies.
- **Spinning and hinges:** loose parts spin with `AssemblyAngularVelocity`
  (radians a second about X, Y and Z; scripts read and set it), and slow
  down rubbing on the ground. `AngularVelocity` (`AngularVelocity`,
  `MaxTorque`, on `Attachment0`) and the older `BodyAngularVelocity`
  (inside a part) keep them spinning. A `HingeConstraint` lets
  `Attachment1`'s part, and everything welded to it, turn only about
  `Attachment0`'s X axis (attachments have an `Orientation` to aim it).
  `ActuatorType` None swings freely under gravity, `Motor` turns at
  `AngularVelocity` with up to `MotorMaxTorque`, and `Servo` turns to
  `TargetAngle` degrees at `AngularSpeed` with up to `ServoMaxTorque`.
  `CurrentAngle` reads the angle. Hinged parts stay on their hinge exactly,
  however fast they turn.
- **Effects:** a `ParticleEmitter` inside a part (or an attachment) sends
  out particles: `Rate`, `Lifetime` and `Speed` (`NumberRange.new(min,
  max)`), `SpreadAngle`, `Size`, `Color`, `Transparency`, `Acceleration`,
  `Drag`, `LightEmission`, and `Emit(count)` for a burst. `ColorSequence` and
  `NumberSequence` (with keypoints) are accepted and use their first value.
  `Fire` (`Size`, `Heat`, `Color`, `SecondaryColor`), `Smoke` (`Color`,
  `Opacity`, `RiseVelocity`, `Size`) and `Sparkles` (`SparkleColor`) are
  ready-made effects. A `Highlight` outlines and tints its `Adornee` (or
  its parent) with `FillColor`, `FillTransparency`, `OutlineColor` and
  `OutlineTransparency`. Effects are drawn while editing too.
- **Remote and bindable functions:** like events, but the caller waits for
  an answer. A `RemoteFunction`'s `InvokeServer(...)` is answered by the
  function a game script sets as `OnServerInvoke(player, ...)`, and
  `InvokeClient(player, ...)` by `OnClientInvoke`. A `BindableFunction`'s
  `Invoke(...)` is answered by `OnInvoke`. The answer may wait itself, errors
  in it reach the caller, and calling before an answer is set waits for one.
- **HttpService:** `JSONEncode(table)`, `JSONDecode(text)`,
  `GenerateGUID(braces?)` and `UrlEncode(text)`. (It doesn't fetch from the
  web.)
- **More value objects:** `ObjectValue` (a link to an instance),
  `Vector3Value` and `Color3Value`, besides the number, string and boolean
  ones.
- **ServerStorage and StarterPlayer:** `ServerStorage` keeps templates out of
  the world; `StarterPlayer.CharacterWalkSpeed` and `CharacterJumpPower` are
  what new characters start with. `workspace.FallenPartsDestroyHeight` (-100)
  is how low loose parts and characters can fall.
- **Input:** `UserInputService.InputBegan` and `InputEnded` fire with an input
  object (`KeyCode`, `UserInputType`) and whether the game's UI already used
  it. `IsKeyDown(Enum.KeyCode.X)` and `IsMouseButtonPressed` check what is
  held.
- **Raycasts:** `workspace:Raycast(origin, direction, params?)` returns the
  first part hit, with `Instance`, `Position`, `Normal`, `Distance` and
  `Material`, or nil. `RaycastParams` has `FilterDescendantsInstances` and
  `FilterType` (`Enum.RaycastFilterType.Exclude` or `Include`). Every part
  shape is hit exactly.
- **Remote and bindable events:** a `RemoteEvent`'s `FireServer(...)` reaches
  `OnServerEvent(player, ...)`; `FireClient(player, ...)` and
  `FireAllClients(...)` reach `OnClientEvent(...)`. A `BindableEvent`'s
  `Fire(...)` reaches its `Event`. With one player on one computer these
  arrive straight away and tables are passed as they are, not copied. In a
  hosted game, remote events go between computers: their values are copied
  (nil, booleans, numbers, strings, tables, instances, `Vector3`, `Vector2`,
  `Color3`, `CFrame`, `UDim`, `UDim2` and enum items; an instance the other
  side can't see arrives as nil), and `RemoteFunction:InvokeServer` waits for
  the host's answer. `FireClient` and `FireAllClients` only work on the host.
- **Saved data:** `DataStoreService:GetDataStore(name)` gives a store with
  `GetAsync`, `SetAsync`, `UpdateAsync`, `IncrementAsync` and `RemoveAsync`.
  Values can be nil, booleans, numbers, strings, and tables of those.
- **Debris:** `Debris:AddItem(instance, seconds)` destroys it later.
- **Part properties and events:** `Position`, `Orientation` (degrees),
  `Size`, `Shape` (`Enum.PartType`), `Color`, `Material` (`Enum.Material`),
  `Anchored`, `CanCollide`, `Transparency`, and the `Touched` event. Enum
  properties also accept a name, as in `part.Material = "Neon"`.
- **Modules:** a `ModuleScript` returns one value; `require(module)` runs it
  once and hands every caller the same value. Modules may `task.wait`.
- **Players:** `game:GetService("Players")` has `PlayerAdded`,
  `PlayerRemoving`, `GetPlayers()` and `GetPlayerFromCharacter(model)`. Each
  `Player` has `Name`, `UserId`, `Character` and a `CharacterAdded` event.
  A `leaderstats` folder of value objects inside a player shows up on the
  leaderboard.
- **Characters:** each player's character is a `Model` named after them with a
  blocky body (`Head`, `Torso`, `LeftArm`, `RightArm`, `LeftLeg`, `RightLeg`
  and two eyes, in Libroxel's own colours with a different shirt per player)
  whose arms and legs swing as it walks, sit in seats and raise a held tool.
  Inside is an invisible `HumanoidRootPart` (the collision box) and a `Humanoid` (`Health`, `MaxHealth`, `WalkSpeed`,
  `JumpPower`, `TakeDamage(amount)` and the `Died` and `HealthChanged(health)`
  events). When health reaches 0 the character dies and respawns after
  `Players.RespawnTime` seconds (5 unless changed). Set
  `Players.CharacterAutoLoads` to false to spawn characters yourself with
  `player:LoadCharacter()`; `player.CharacterRemoving` fires before a
  character is removed.
- **Welds:** a `WeldConstraint` with `Part0` and `Part1` (and `Enabled`)
  holds two parts together while playing. Parts joined by welds move as one
  assembly: unanchor them and they fall, slide and get pushed together. In
  the editor, Insert → WeldConstraint with a part selected welds it to the
  part it touches, and Part0/Part1 can be picked from a list. Welds (and
  other links between saved instances) are saved in places, and `Clone`
  points a copy's welds at the copied parts.
- **Seats:** a character that touches a `Seat` or `VehicleSeat` sits in it
  and moves with it; jumping gets up. `seat.Occupant` is the humanoid in it,
  `seat:Sit(humanoid)` sits one down, `Disabled` keeps everyone off, and the
  humanoid has `Sit`, `SeatPart` and the `Seated(active, seat)` event (set
  `Sit` to false to stand up).
- **Vehicles:** while you sit in a `VehicleSeat`, W/S and A/D set its
  `Throttle` and `Steer` (scripts can set them too). It pushes the unanchored
  assembly it's welded into along its front at up to `MaxSpeed` (studs/s),
  getting there as fast as its `Torque` allows, and turns at up to
  `TurnSpeed` radians a second while moving.
- **Gravity:** `workspace.Gravity` (196.2 studs/s² unless changed).
- **NPCs:** any `Model` in the workspace with a `Humanoid` and an unanchored
  `HumanoidRootPart` is a character. `Humanoid:MoveTo(position)` walks it
  there and fires `MoveToFinished(reached)` (it gives up after 8 seconds),
  setting `Humanoid.Jump = true` makes it jump, and its other parts follow
  the root. `MoveTo` works on the player's character too, until they steer.
- **Teams:** `Team`s go in the `Teams` service and have `TeamColor` and
  `AutoAssignable`. New players join the smallest auto-assignable team;
  `player.Team` and `player.Neutral` change it. A `SpawnLocation` with
  `Neutral = false` only spawns players whose team colour matches its
  `TeamColor`. `Teams:GetTeams()` and `team:GetPlayers()` list them, and the
  leaderboard groups players by team.
- **Random numbers:** `Random.new(seed?)` with `NextNumber(min?, max?)` and
  `NextInteger(min, max)`.
- **Services:** `RunService.Stepped(time, dt)`, `Heartbeat(dt)` and
  `RenderStepped(dt)` fire each frame in that order. While play testing on
  one computer `IsClient`, `IsServer` and `IsStudio` all return true; in a
  hosted game the host is the server and a joined computer is a client
  (`IsStudio` is false for both). Scripts start when they are in Workspace or ServerScriptService.

A kill brick, for example:

```lua
script.Parent.Touched:Connect(function(hit)
	local humanoid = hit.Parent and hit.Parent:FindFirstChild("Humanoid")
	if humanoid then
		humanoid.Health = 0
	end
end)
```

Each script runs in its own sandboxed environment; the shared libraries are
read-only. A script that errors is reported in Output and never stops the
others.

A coin that counts on the leaderboard, using a module in ReplicatedStorage:

```lua
local Players = game:GetService("Players")
local Stats = require(game:GetService("ReplicatedStorage").Stats)

script.Parent.Touched:Connect(function(hit)
	local player = Players:GetPlayerFromCharacter(hit.Parent)
	if player then
		Stats.add(player, "Coins", 1)
	end
end)
```

A door that slides open when clicked:

```lua
local TweenService = game:GetService("TweenService")
local door = script.Parent

door.ClickDetector.MouseClick:Connect(function(player)
	local goal = { CFrame = door.CFrame + Vector3.new(0, 6, 0) }
	TweenService:Create(door, TweenInfo.new(0.6), goal):Play()
end)
```

**Not yet:** tool animations, sounds, and terrain and voxels. Physics is simple: anchored parts can be rotated and shaped
(ramps and balls work), but loose parts collide as the upright box around
them and don't tip over by themselves (they spin only when something spins
them). A hinge arm is carried by its hinge, so it blocks things but doesn't
push them, and wheels on hinges don't drive a car. Characters climb steps up to 0.6 studs.

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md)
first, especially the **clean-room rules**, and follow our
[Code of Conduct](CODE_OF_CONDUCT.md).

## License

Libroxel is dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
