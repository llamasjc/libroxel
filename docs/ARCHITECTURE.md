# Architecture

```
 Luau scripts ──▶ libroxel-script ──▶ libroxel-instance ◀── libroxel-engine ◀── libroxel-editor
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
  into the services instead of creating new ones. Links between instances
  (like a player's `Character`) are runtime-only and are not saved.
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
- **Physics.** Unanchored parts are upright boxes. They fall, are moved one
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
- **GUI layout.** A `UIListLayout` replaces its siblings' positions with a
  column or row; a `UICorner` sets the rounding. While playing, the text
  box being typed into gets an egui text field over it, which writes into
  `Text`; focus changes go through the runtime so `Focused`, `FocusLost`
  and `CaptureFocus` agree with the editor.
- **Threading.** The Luau VM is single-threaded, so the engine state is a
  Bevy non-send resource and the tree is shared as `Rc<RefCell<_>>`.

## Known limits

- The ECS sync does a full pass over the workspace each frame; it will move
  to the change journal once worlds get big.
- Loose parts never rotate, and cylinders collide as boxes.
- No networking.
