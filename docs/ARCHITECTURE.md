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
- **Services.** Every tree has a root and three locked services: Workspace
  (the world), Players (one Player per person in the game, made by the
  engine) and ReplicatedStorage (modules and templates). Saved places merge
  into the services instead of creating new ones. Links between instances
  (like a player's `Character`) are runtime-only and are not saved.
- **Modules.** `require` is written in Luau so that a module can yield. It
  caches each module's single return value and detects require loops.
- **Play sessions.** Pressing Play clones the tree, starts every `Script`,
  adds the local Player (firing `PlayerAdded`) and spawns their character
  (firing `CharacterAdded`). Each frame runs: waiting threads and `Heartbeat`,
  character movement, physics, `Touched` events, then deaths and respawns.
  Stop throws the session away and restores the clone.
- **Physics.** Unanchored parts are upright boxes. They fall, are moved one
  axis at a time and pushed out of anything with `CanCollide`, and step up
  ledges up to 0.6 studs. Anchored parts can be rotated and shaped: boxes
  turned by right angles stay on the fast axis-aligned path, tilted boxes and
  wedges are resolved with a separating-axis test, and balls with a sphere
  test, pushing out along the shortest direction (straight up on gentle
  slopes, so characters don't slide). Touches are pairs that start
  overlapping, where at least one part can move.
- **Threading.** The Luau VM is single-threaded, so the engine state is a
  Bevy non-send resource and the tree is shared as `Rc<RefCell<_>>`.

## Known limits

- The ECS sync does a full pass over the workspace each frame; it will move
  to the change journal once worlds get big.
- Orientation is a plain vector of degrees; there is no `CFrame` yet.
- Loose parts never rotate, and cylinders collide as boxes.
- No networking.
