# Architecture (Phase 0)

```
 Luau scripts ──▶ libroxel-script ──▶ libroxel-instance ◀── libroxel-proto (Bevy)
   (sandboxed)     names.rs maps          InstanceTree        mirrors parts into
                   script names to        + change journal    entities each frame
                   engine names
```

- **The instance tree is the source of truth.** Scripts change the tree; the
  Bevy side reads it and mirrors parts onto entities (`PartLink`,
  `Transform`, `PartAppearance`). Rendering never writes back.
- **The naming layer.** `libroxel-instance` uses internal enums
  (`ClassKind`, `Property`). `libroxel-script/src/names.rs` is the only place
  that knows the strings scripts use, so the public API can be renamed or
  versioned without touching the engine.
- **Ids are generational.** A destroyed instance stays readable (its parent
  is `nil` and locked) until `purge_destroyed`; after that its id never
  resolves again, even when the slot is reused.
- **Threading.** The Luau VM is single-threaded, so the script host is a Bevy
  non-send resource and the tree is shared as `Rc<RefCell<_>>`.
- **Frame order.** `Update`: scripts' `Heartbeat` handlers run, then parts
  are synced into the ECS. `PostUpdate` (in the binary): meshes and
  materials are attached or refreshed.

## Known Phase 0 limits

- The ECS sync does a full pass over the workspace each frame; it will move
  to the change journal once there are many parts.
- No `task.wait`/coroutine scheduling, no script execution time limits yet.
- No physics: `Anchored` is stored but has no effect yet.
- No networking or persistence.
