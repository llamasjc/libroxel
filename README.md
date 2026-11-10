# Libroxel

**Build voxel worlds together, and script them in Luau.**

Libroxel is an open-source, multiplayer game-creation platform. Worlds are
built from blocks and parts, behaviour is written in [Luau](https://luau.org),
and the engine runs on [Bevy](https://bevy.org).

> **Status: Phase 0 (foundation).** There is a working instance tree, a
> sandboxed Luau runtime, and a prototype window where a script moves a part.
> Nothing here is stable yet.

## Try the prototype

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
cargo run -p libroxel-proto                       # runs the bundled demo script
cargo run -p libroxel-proto -- path/to/script.luau  # runs your own script
cargo test --workspace                            # runs every test, no GPU needed
```

The first build compiles Bevy and takes a few minutes. After that it is quick.

The demo script, [`move_part.luau`](crates/libroxel-proto/scripts/move_part.luau),
looks like this:

```lua
local RunService = game:GetService("RunService")

local mover = Instance.new("Part")
mover.Size = Vector3.new(2, 2, 2)
mover.Color = Color3.fromRGB(255, 140, 0)
mover.Parent = workspace

local elapsed = 0
RunService.Heartbeat:Connect(function(dt)
	elapsed += dt
	mover.Position = Vector3.new(math.cos(elapsed) * 6, 1, math.sin(elapsed) * 6)
end)
```

## Layout

| Crate | What it is |
| --- | --- |
| [`libroxel-instance`](crates/libroxel-instance) | The instance tree: classes, typed properties, parenting, destroy, and a change journal. No engine dependencies. |
| [`libroxel-script`](crates/libroxel-script) | The sandboxed Luau runtime (via `mlua`) bound to the instance tree. Every name scripts can see lives in [`names.rs`](crates/libroxel-script/src/names.rs). |
| [`libroxel-proto`](crates/libroxel-proto) | The Phase 0 Bevy prototype that mirrors parts into the ECS and renders them. |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit together.

## Scripting API

Scripts use a familiar scene-graph style: an `Instance` tree rooted at
`game`, a `workspace`, value types like `Vector3` and `Color3`, services from
`game:GetService`, and signals with `:Connect`. What exists today:

- **Globals:** `game`, `workspace`, `Instance.new(class, parent?)`,
  `Vector3.new/zero/one`, `Color3.new/fromRGB`, `print`, `warn`.
- **Classes:** `Part`, `Model`, `Folder`, `Script`, plus the built-in `Workspace`.
- **Instance members:** `Name`, `ClassName`, `Parent`, `FindFirstChild`,
  `FindFirstChildOfClass`, `GetChildren`, `GetDescendants`, `IsA`,
  `IsDescendantOf`, `GetFullName`, `Destroy`, and `instance.ChildName`.
- **Part properties:** `Position`, `Size`, `Color`, `Anchored`, `Transparency`.
- **Services:** `RunService.Heartbeat`, fired once per frame with the frame time.

Each script runs in its own sandboxed environment; the shared libraries are
read-only.

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
