# Libroxel

**Build voxel worlds together, and script them in Luau.**

Libroxel is an open-source, multiplayer game-creation platform. Worlds are
built from blocks and parts, behaviour is written in [Luau](https://luau.org),
and the engine runs on [Bevy](https://bevy.org).

> **Status: early.** There is an editor where you can build a world, write
> scripts and press Play to test it with a character. Nothing is stable yet.

![The Libroxel editor](docs/editor.png)

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
cargo run -p libroxel-proto -- my_game.lrx        # play a saved game without the editor
cargo test --workspace                            # every test, no GPU needed
```

The first build compiles Bevy and takes a while. After that it is quick.

## The editor

- **File → Open sample obby** loads a small obstacle course with lava,
  fading platforms, a lift and a finish pad. Press **▶ Play** (or F5) to try it.
- **Explorer** (left) shows the instance tree. Click to select, double-click
  a script to edit it, drag one item onto another to move it inside, and
  right-click for Insert, Rename, Duplicate and Delete.
- **Properties** (right) edits the selection: name, position, size, colour,
  anchored, can-collide, transparency, and so on.
- **Viewport:** hold the right mouse button and use WASD/QE to fly (Shift is
  faster), scroll to move forward. Click a part to select it, then drag the
  arrows to move it or switch to Resize. Snap sets the grid step.
- **Play / Stop:** Play spawns your character at the first SpawnLocation and
  starts every Script. Walk with WASD, jump with Space, turn the camera with
  the right mouse button. Stop (or Esc) puts the world back as it was.
- **Output** (bottom) shows `print`, `warn` and script errors. While playing
  you can type Luau into its command bar and press Enter.
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
  `Vector3.new/zero/one`, `Color3.new/fromRGB`, `print`, `warn`, `time()`,
  `task.wait`, `task.spawn`, `task.delay`, and `wait`.
- **Classes:** `Part`, `SpawnLocation`, `Model`, `Folder`, `Script`,
  `Humanoid`, plus the built-in `Workspace`.
- **Instance members:** `Name`, `ClassName`, `Parent`, `FindFirstChild`,
  `FindFirstChildOfClass`, `WaitForChild`, `GetChildren`, `GetDescendants`,
  `IsA`, `IsDescendantOf`, `GetFullName`, `Clone`, `Destroy`, and
  `instance.ChildName`.
- **Part properties and events:** `Position`, `Size`, `Color`, `Anchored`,
  `CanCollide`, `Transparency`, and the `Touched` event.
- **Characters:** each player is a `Model` named `Player` with a
  `HumanoidRootPart` and a `Humanoid` (`Health`, `MaxHealth`, `WalkSpeed`,
  `JumpPower`). Setting `Health` to 0 kills the character; it respawns after
  two seconds.
- **Services:** `RunService.Heartbeat`, fired once per frame with the frame time.

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

**Not yet:** part rotation, `CFrame`, GUIs, sounds, terrain and voxels,
multiplayer, and data saving. Physics is simple: parts are axis-aligned
boxes that fall and stack, without spinning.

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
