# Architecture

```
 Luau scripts ──▶ libroxel-script ──▶ libroxel-instance ◀── libroxel-engine ◀── libroxel (app)
   (sandboxed)     names.rs maps          InstanceTree         play sessions,       panels, gizmo,
                   script names to        + change journal     physics, character,  undo, files
                   engine names           + .lrx files         Bevy rendering
```

- **The instance tree is the source of truth.** Scripts, physics and the
  editor all change the tree; the Bevy side mirrors parts onto entities
  (`PartLink`, `Transform`, `PartAppearance`) each frame. Rendering never
  writes back.
- **The naming layer.** `libroxel-instance` uses internal enums
  (`ClassKind`, `Property`) and stable file names for saving.
  `libroxel-script/src/names.rs` is the only place that knows the strings
  scripts use, so the public API can be renamed or versioned without
  touching the engine or breaking saved games.
- **Ids are generational.** A destroyed instance stays readable (its parent
  is `nil` and locked) until `purge_destroyed`; after that its id never
  resolves again, even when the slot is reused.
- **Scripts are threads.** Every script, signal handler and `task.spawn`
  runs in its own Luau thread. `task.wait` yields; the scheduler resumes the
  thread on the right frame. One failing thread never stops the others.
- **Services.** Every tree has a root and locked services: Workspace (the
  world), Players (one Player per person in the game, made by the engine),
  ReplicatedStorage (modules and templates), ServerScriptService (scripts
  outside the world), StarterGui (screen GUIs copied to each player) and
  Lighting (time of day). Saved places merge
  into the services instead of creating new ones. Links between saved
  instances (like a weld's `Part0`) are saved as numbers: each instance
  something links to gets a `ref_id`, and links are `{"ref": n}`. Links to
  things that aren't saved (like a player's `Character`) are dropped.
  `clone_subtree` points links inside the copy at the copies.
- **Modules.** `require` is written in Luau so that a module can yield. It
  caches each module's single return value and detects require loops.
- **Play sessions.** Pressing Play clones the tree, starts every `Script` in
  Workspace and ServerScriptService, adds the local Player (firing
  `PlayerAdded`), copies StarterGui into their `PlayerGui` (starting the
  `LocalScript`s there) and spawns their character (firing `CharacterAdded`).
  Each frame runs: `Changed` events for whatever changed since the last
  frame, waiting threads, tweens, `Heartbeat`, character movement, physics,
  `Touched` events, then deaths and respawns. Stop throws the session away
  and restores the clone.
- **Change events.** The tree records every property and parent change in
  its journal. The session drains it once a frame and fires `Changed`,
  `GetPropertyChangedSignal` and `ChildAdded` for instances that scripts are
  listening to, so events never run in the middle of another script.
- **Tweens** are owned by the host, not by Luau threads: each frame the
  runtime advances them, eases the progress and writes the in-between value,
  then fires `Completed`. A new tween on the same property takes it over.
- **GUIs** are instances like any other. The editor lays them out each frame
  from `Position`, `Size` and `AnchorPoint` (scale of the parent plus pixel
  offset) and draws them with egui over the 3D view; clicks on buttons and
  `ClickDetector` parts go back to the session, which fires the events.
- **CFrames.** Parts store `Position` and `Orientation` (degrees, applied Y,
  then X, then Z). `CFrame` is built from and written back to those two, so
  saved games stay readable.
- **Physics.** Unanchored parts collide as the axis-aligned box around them
  (characters' root parts as their own size, whichever way they face). They
  can spin: physics keeps an angular velocity for each, turns its
  `Orientation` by it each step, and slows it on the ground. They fall, are moved one
  axis at a time and pushed out of anything with `CanCollide`, and step up
  ledges up to 0.6 studs. Anchored parts can be rotated and shaped: boxes
  turned by right angles stay on the fast axis-aligned path, tilted boxes and
  wedges are resolved with a separating-axis test, and balls with a sphere
  test, pushing out along the shortest direction (straight up on gentle
  slopes, so characters don't slide). Touches are pairs that start
  overlapping, where at least one part can move.
- **Input and prompts.** The Bevy side passes key presses and releases to
  the session, and the editor passes mouse buttons (marked as used when they
  land on a GUI). The session forwards them to `UserInputService` and keeps
  track of the nearest proximity prompt in reach, how long its key has been
  held, and fires `Triggered` once the hold is long enough.
- **Raycasts** live in `libroxel-instance::geometry`, so scripts and the
  editor's picking share one exact test per shape (box planes, a wedge's
  slope, a sphere, a cylinder along X), done in the part's own space.
- **Saved data.** Data stores are an in-memory map of store, key and JSON
  value inside the runtime. A session can start with saved JSON and hands it
  back when it stops (after `PlayerRemoving`, so games can save on leave);
  the editor keeps it in a file beside the game.
- **Attributes and tags** are stored on each instance (a sorted map and a
  sorted list), journalled like property changes, and saved in `.lrx` files
  as an `attributes` object (typed values carry a `kind`) and a `tags` array.
  `CollectionService` reads them straight from the tree.
- **Tools.** At each spawn the session clears the player's Backpack, clones
  StarterPack's tools into it and starts their scripts. The hotbar is the
  backpack's tools plus the one in the character. Holding a tool reparents
  it into the character; the change journal sees that move and fires
  `Equipped` or `Unequipped`. Each frame the session turns the character to
  face where it walks and places the held tool's `Handle` in its hand. The
  editor sends the mouse ray every frame, so `Mouse.Hit` and `Target` are a
  raycast that skips the player's own character.
- **Billboards** are drawn by the editor like screen GUIs, but each one is
  projected from its part (plus `StudsOffset`), sized in studs by the
  distance to the camera, and hidden past `MaxDistance` or, unless
  `AlwaysOnTop`, behind other parts.
- **Camera.** A play session puts a `Camera` in the workspace. While its
  `CameraType` is `Custom` the view orbits the character and writes where it
  is into the camera each frame; when scripts make it `Scriptable`, the view
  follows the camera's `CFrame` instead. `FieldOfView` sets the Bevy
  projection, and the editor tells scripts the view's size.
- **Velocities** stay in the physics engine. Scripts' writes to
  `AssemblyLinearVelocity` and `ApplyImpulse` calls queue requests that the
  session applies before physics runs; after the step it hands scripts a
  copy of every velocity to read. A character keeps a push it gets in the
  air instead of the walk speed replacing it.
- **Explosions** go off the first step they are in the workspace: the
  session finds parts whose box is within the blast radius, fires `Hit`,
  kills characters and throws loose parts, and keeps a short-lived flash
  for the renderer.
- **Lights** are mirrored onto Bevy point and spot lights at their part.
- **GUI layout.** A `UIListLayout` or `UIGridLayout` replaces its siblings'
  positions with a column, row or grid; a `UICorner` sets the rounding,
  `UIPadding` shrinks the area children are placed in, and `UIScale` and
  `UIAspectRatioConstraint` change an object's own size after `Size` is
  worked out. Siblings are drawn in `ZIndex` order. A `ScrollingFrame`
  places its children on a canvas of `CanvasSize`, shifted by
  `CanvasPosition` and clipped to the frame; clicks and hovering outside a
  clip don't reach what is hidden. The editor tells the session which GUI
  object is under the pointer each frame, and the session fires
  `MouseLeave` and `MouseEnter` for the objects (and their GUI ancestors)
  the pointer left and entered. While playing, the text
  box being typed into gets an egui text field over it, which writes into
  `Text`; focus changes go through the runtime so `Focused`, `FocusLost`
  and `CaptureFocus` agree with the editor.
- **NPCs** live in `libroxel-engine/src/npc.rs`. Before physics, every
  humanoid model other than the player's character is driven with the same
  walk code as the player (`character::drive`), steered toward its `MoveTo`
  target. After physics its other parts are carried along with the root
  (anchored and non-colliding) and turned to face where it walks; arrived,
  timed-out and cut-short walks fire `MoveToFinished`, and health reaching 0
  fires `Died`.
- **Teams.** Before `PlayerAdded` fires the session puts the player on the
  auto-assignable team with the fewest players. Spawning prefers a
  non-neutral spawn of the player's team colour, then a neutral spawn.
- **Welds** live in `libroxel-engine/src/weld.rs`. Each step, enabled
  welds in the workspace are grouped into assemblies (union-find). Each
  assembly has one root, the part physics moves: its `HumanoidRootPart`,
  else an anchored part, else the biggest. The other parts are carried: they
  act anchored during the step (so things bump into them), the pairs inside
  one assembly ignore each other, and after the step each carried part is
  put back at its offset from the root. The offset is taken when the part
  joins, or again if a script moved it since.
- **Chess** is its own crate, `libroxel-chess`, with no other
  dependencies: the board (squares 0 to 63 from a1), move generation
  checked against standard perft counts, notation, FEN and PGN, how games
  end, clocks, Elo ratings, and the computer players (alpha-beta with
  iterative deepening, quiescence and a transposition table; weaker
  players look less far and misjudge on purpose). The Hub
  (`libroxel-hub/src/chess.rs`) uses the same crate to check every move of
  an online game and keeps the clocks itself: a game stores its moves and
  each side's time left, and the side to move's clock is worked out from
  when the last move came. Every player action is one signed request to
  `POST /chess`, answered with that player's whole lobby; the app's Chess
  tab (`chess.rs`, pieces in `chess_pieces.rs`) sends them from one thread
  so their signed times always go up, and asks again about once a second
  while a game is on.
- **Speed.** Physics uses a broadphase: in worlds of more than 64 parts,
  each box (grown by how far it can move this step) goes into a grid of
  8-stud cells, and parts are only compared with others sharing a cell;
  very large boxes such as baseplates are compared with everything.
  Every instance has a *revision* that changes whenever a property does
  (from one counter shared by all trees, so undo snapshots never reuse a
  number); the renderer skips parts whose revision it has already shown.
  `cargo run -p libroxel-engine --example bench -- 2000` times frames in a
  world of 2000 parts.
- **Bodies.** A character's `HumanoidRootPart` is the invisible collision
  box; the visible body (`character::build_rig`) is anchored, non-colliding
  parts that `pose_rig` places around the root every frame, swinging the
  limbs with the walking speed. NPC models with the same parts are posed the
  same way.
- **Scripts stop when destroyed.** Every connection and waiting thread
  remembers the script instance that made it. When that script is destroyed
  (as a GUI's local scripts are when it is replaced on respawn), its
  connections are dropped and its threads never resume.
- **Seats and vehicles** live in `libroxel-engine/src/seat.rs`. A sitting
  character is carried like a welded part: physics holds it still and lets
  it pass through the seat's assembly, and after the step it is put on the
  seat. Before physics, every vehicle seat with a `Throttle` or `Steer`
  sets the velocity of its assembly's root and turns the root's yaw; the
  welds then carry the rest of the vehicle (and the seat) round with it.
  The player's raw W/S and A/D keys come in with the character input as
  `throttle` and `steer`.
- **Movers and constraints** live in `libroxel-engine/src/movers.rs`. After
  the welds are worked out and before the physics step, each enabled
  `LinearVelocity`, `VectorForce`, `AlignPosition`, `RopeConstraint`,
  `SpringConstraint` and legacy `Body*` mover changes the velocity of the
  assembly root its attachment's part belongs to, using the assembly's
  whole mass. Each one aims for the velocity the body should have at the
  end of the step (physics adds gravity during it, so movers that hold
  things up add it back). Springs, `AlignPosition` and `BodyPosition` are
  solved implicitly, so stiff ones don't blow up; ropes cancel stretching
  and close a share of any overshoot each step. Visible ropes and springs
  are drawn by `view::sync_ropes` as thin rods. `AngularVelocity` and
  `BodyAngularVelocity` change the root's angular velocity the same way,
  using a rough moment of inertia for the assembly.
- **Terrain** is a `Terrain` instance whose cells live beside its
  properties (`libroxel-instance/src/voxels.rs`): chunks of 16³ one-byte
  cells, a code per material, saved and sent as run-length text.
  `InstanceTree::edit_voxels` bumps the terrain's revision and journals a
  `VoxelsChanged`, so the replicator resends the cells (as `x`) and the
  renderer notices. Physics adds the cells near each moving body as anchored
  boxes (runs along X merged) that can be climbed one stud at a time
  (`TERRAIN_STEP`). Water cells aren't solid: a loose body whose middle is
  in water floats up against drag, and a character's root hangs there while
  `character::drive` swims it. Rays step through cells with a grid walk
  and pass through water.
  `terrain_view.rs` draws a mesh per chunk and material from the faces that
  look onto air, rebuilding only chunks whose cells (or neighbours) changed.
- **Hinges** live in `libroxel-engine/src/hinge.rs`. Each enabled
  `HingeConstraint` has a base (Attachment0's part, unless only the other
  side is anchored) and an arm (the other part and everything welded to it).
  The hinge keeps its own angle and speed: gravity's torque about the axis
  and the actuator (motor or servo, limited by its torque over the arm's
  inertia) change the speed before the step. Arms are held still during the
  step like welded parts, then placed afterwards at the base's frame times
  Attachment0's frame turned by the angle, times each arm part's pose taken
  when the hinge started (so nothing drifts). Bases are placed before the
  arms hanging from them, so hinges can chain. A script moving an arm part
  starts the hinge again from where it is.
- **Pictures** are found and decoded by `libroxel-engine/src/pictures.rs`
  (folders set by the editor, plus built-in PNGs in `engine/assets`), for
  both egui images and decals. `view::sync_decals` draws each `Decal` and
  `Texture` as a square on its part's side (`decal_transform`), lifted a
  hundredth of a stud with a depth bias; textures tile with a repeating
  sampler and the material's `uv_transform`.
- **Sounds** live in `libroxel-engine/src/sounds.rs`. Scripts only change
  `Playing` (and `Play`/`Stop` queue a restart); each frame `play_sounds`
  turns the sounds under Workspace and Players into Bevy audio players,
  pausing, resuming and adjusting them, and tells the session when one has
  finished so `Ended` fires. Built-in sounds are synthesised into WAV bytes
  in code. Without Bevy's audio plugin the system does nothing.
- **Tipping:** after the substeps, `Physics::tip` looks at each resting
  loose part's lowest corners and the tops of what is just under it. If its
  middle is outside where those meet, gravity spins it about the nearest
  edge (and swings its middle round it); if it is held up and within 8° of
  lying flat, it is laid exactly flat. Parts a spinner drives
  (`Physics::drive_spin`), welded assemblies, balls and characters are skipped.
- **Hinge pushes and wheels:** `Hinges::before_physics` tells physics how
  each arm part moves (`Physics::set_motion`: a pivot, a spin and the
  base's velocity). Physics pushes loose bodies out of moving arms and gives
  them at least the arm surface's speed along the push. Round arms on a
  loose base cast a ray down; where they reach the ground they hold the
  base up and steer its velocity towards the wheels' rolling speed.
- **Tool swings:** the script runtime notes each `Activated`; the session
  gives the holder's avatar a short `slash` timer, and `character::slash_angle`
  turns it into extra right-arm tilt that the pose and the tool handle share.
- **Effects** live in `libroxel-engine/src/effects.rs` and only exist on
  screen. Each particle is a small entity; each emitter has four materials
  from the start of a particle's life to its end, and particles switch
  material as they age, so fades and colour changes cost nothing per
  particle. `ParticleEmitter:Emit` requests go from the runtime to the
  session to `EngineState::emits`. A highlight draws two copies of each part
  it covers: a slightly bigger see-through fill, and a bigger one showing
  only its inside faces, which looks like an outline.
- **Events from the change journal.** `fire_changes` turns reparenting into
  `ChildAdded`/`ChildRemoved` on the parents and `DescendantAdded`/
  `DescendantRemoving` on every watched ancestor (for the moved instance
  and everything in it), destruction into `Destroying`, and a humanoid's
  `Health` changing into `HealthChanged`. Physics reports pairs that stopped
  touching for `TouchEnded`.
- **Players.** The session keeps an `Avatar` for each player: their player
  object, GUI container, backpack and hotbar, character, prompt being held,
  seat and respawn timer. Every step drives each character from its own
  input, so the same code serves one player or many; NPC code leaves all
  players' characters alone.
- **Networking** lives in `libroxel-engine/src/net.rs` and only uses the
  standard library and `serde_json`. A hosted session is an ordinary
  session (scripts, physics, the host's own player) that also accepts TCP
  connections. Messages are one JSON object per line. A joining computer
  says hello with a name and gets a player of its own (its input arrives
  in `input` messages and drives its character on the host). After each
  step, the `Replicator` walks the shared services (Workspace, Players,
  ReplicatedStorage, StarterGui, Lighting, Teams; never cameras or player
  GUIs), skips instances whose revision hasn't changed (revisions now also
  change with attributes and tags), and sends created, updated and removed
  instances as operations named by the host's ids. A newcomer gets
  everything at once. Server `Script` source is sent empty. A joined
  session has a fresh tree, maps the host's ids to its own, applies the
  operations (links last, since they may point ahead), runs no `Script`s
  and no physics, and starts `LocalScript`s in its copy of StarterGui and
  in its tools as they arrive. Clicks on click detectors, prompts, hotbar
  keys and the mouse button over the world go to the host, which checks
  reach and fires the events there.
- **Who moves a joined player's character.** While it is alive and not
  sitting, the joined computer does, like everything on one computer: it
  runs physics for that one root part (every other loose part is held where
  the host put it) and sends a `move` (position, facing, velocity) each
  step. The host puts the root there and *holds* it for its own physics
  step: the root doesn't move, but it still touches things, so coins and
  kill bricks work on the host. Host updates to that character's position
  are ignored on its own computer. If something on the host moves the
  character itself (a script, say), the host numbers a `teleport`, sends
  it, and ignores moves numbered before it until the joined computer
  catches up. A sitting or dead character is moved by the host, and its
  computer shows where the host puts it.
- **Remote events across computers.** The script runtime knows its role
  (`Solo`, `Server` or `Client`) and the local player. Remote calls aimed
  at the other side become `NetMessage`s with their arguments as JSON
  (instances tagged `{"$i": id}`); the engine rewrites the ids for the
  other side and delivers them there. `InvokeServer` on a client sends the
  call and yields until the answer comes back; the host runs
  `OnServerInvoke` in its own thread so it may wait too.
- **Threading.** The Luau VM is single-threaded, so the engine state is a
  Bevy non-send resource and the tree is shared as `Rc<RefCell<_>>`.

## Known limits

- The ECS sync still lists the workspace's parts each frame, though it
  only copies the ones whose revision changed.
- Loose parts never rotate, and cylinders collide as boxes.
- A welded assembly collides with the world through its root part only;
  its carried parts stop other things but don't stop the assembly.
- The host sends everyone the whole shared world, every step something
  changes; there is no interest management or compression.
- `ParticleEmitter:Emit` bursts and velocities aren't sent to joined
  computers.
