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
- **Play sessions.** Pressing Play clones the tree, spawns a character and
  starts every `Script`. Each frame runs: waiting threads and `Heartbeat`,
  character movement, physics, `Touched` events, then deaths and respawns.
  Stop throws the session away and restores the clone.
- **Physics.** Parts have no rotation yet, so every part is an axis-aligned
  box. Unanchored parts fall, and are moved one axis at a time and pushed
  out of anything with `CanCollide`. Touches are pairs that start
  overlapping, where at least one part can move.
- **Threading.** The Luau VM is single-threaded, so the engine state is a
  Bevy non-send resource and the tree is shared as `Rc<RefCell<_>>`.

## Known limits

- The ECS sync does a full pass over the workspace each frame; it will move
  to the change journal once worlds get big.
- No part rotation, so no `CFrame` yet.
- No script execution time limits yet: an endless loop without `task.wait`
  freezes the game.
- No networking.
