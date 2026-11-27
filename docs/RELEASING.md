# Releasing PlayLibre

`.github/workflows/release.yml` builds the PlayLibre app for every platform
and attaches the builds to a GitHub Release whenever a version tag is pushed.

| File | Platform | What's inside |
| --- | --- | --- |
| `playlibre-<version>-windows-x86_64-setup.exe` | Windows 10/11 (x86-64) | a setup wizard with the PlayLibre icon: Start menu and desktop shortcuts, PATH, `playlibre://` links, and an uninstaller in Settings > Apps |
| `playlibre-<version>-macos-universal.dmg` | macOS 11+ (Apple silicon and Intel) | `PlayLibre.app` with its icon, on a window that shows dragging it onto Applications |
| `playlibre-<version>-linux-x86_64.AppImage` | Linux (x86-64, glibc 2.35+) | the app for any distribution: make it executable and open it; it adds itself to the app menu with its icon |
| `playlibre-<version>-linux-x86_64.deb` | Debian and Ubuntu | the same app as a package, with its menu entry and icon (`sudo apt install ./…deb`, or open it in Software) |
| `playlibre-<version>-windows-aarch64-setup.exe` | Windows 11 on ARM | the same setup wizard, built for ARM (when that build worked) |
| `playlibre-<version>-linux-aarch64.AppImage`, `.deb` | Linux on 64-bit ARM (glibc 2.35+) | the same app for ARM Linux, such as a Raspberry Pi 4 or 5 on a 64-bit system (when that build worked) |
| `playlibre-hub-linux-x86_64` | Linux servers (x86-64) | the Hub on its own, for the official Hub server |
| `setup-hub.sh`, `update-hub.sh` | | set a Hub server up, and update it (see [OFFICIAL_HUB.md](OFFICIAL_HUB.md)) |
| `SHA256SUMS.txt` | | checksums of the files above |

Each installer holds a single program: the built-in pictures and starter games are compiled
into it, so nothing else needs to sit next to it.

## Publishing a release

1. In `Cargo.toml`, change the version to the one you're about to tag: under
   `[workspace.package]` and on each `playlibre-…` line under
   `[workspace.dependencies]` (search for the old number). Run `cargo check`
   so `Cargo.lock` follows, add a section to `CHANGELOG.md`, commit, and
   push to `main`. The released app takes its version from the tag either
   way; this keeps builds from source and logs saying the right number.
2. Tag the commit and push the tag:

   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```

3. Watch the **Release** run on the repository's Actions tab. It takes roughly
   15 to 30 minutes, mostly compiling Bevy. When it finishes, the release
   appears under **Releases** with all the files attached and notes generated
   from the commits since the previous tag.

A tag with a hyphen, such as `v0.2.0-beta.1`, is published as a pre-release.

The official Hub server picks up each new (non-pre-release) release within
about 10 minutes and updates itself.

## The Hub server

Every release also carries the Hub for servers, next to the app. The
official Hub server usually builds itself from `main` instead (see
[OFFICIAL_HUB.md](OFFICIAL_HUB.md)), so a Hub fix only needs a push to
`main`, not a release.

## Why releases take a while, and the cache

A full release compiles the whole engine (Bevy, physics, Luau) for Linux,
Windows and macOS (for Apple silicon and Intel, built side by side and joined into one app), with every
optimisation on. Already-built libraries are cached, but GitHub only lets a
tag's run use caches saved on `main`, not ones an earlier tag saved. So
**Release** also runs on every push to `main`, without publishing, to keep
those caches warm: a release tagged soon after a push to `main` then mostly
rebuilds only PlayLibre's own code.

To try the builds without publishing anything, open **Actions → Release → Run
workflow**. The files are attached to that run as downloadable artifacts.

If a release goes wrong, delete it and its tag on GitHub, fix the problem, and
push the tag again.

## Running a downloaded build

- **Windows:** unzip and double-click `playlibre.exe`. SmartScreen may warn
  that the app is from an unknown publisher, because it isn't code-signed:
  choose **More info → Run anyway**.
- **macOS:** unzip and open `PlayLibre.app`. It is not notarized by Apple, so
  the first time, open it and click **Open Anyway** in **System Settings →
  Privacy & Security**, or run
  `xattr -dr com.apple.quarantine PlayLibre.app`.
- **Linux:** `tar xzf playlibre-*.tar.gz` and run `./playlibre`. It needs a
  Vulkan driver, plus Wayland or X11, which desktop installs already have.

## Not done yet

- Code signing for Windows and notarization for macOS (both need paid
  certificates; add them as repository secrets when there are some).
- The ARM builds for Linux and Windows haven't been tried on ARM hardware
  yet. They're optional: a release still comes out without them if they
  fail to build, and ARM copies update only to ARM builds
  (`windows-aarch64`, `appimage-aarch64` and `deb-aarch64` in
  `latest.json`).

## Nothing changes until a release is finished

The release is made as a draft, every file is put on it, and only then is it
published, in one step. Until then the website's Download page, the app's
download links and Hub servers keep using the release before it, so a
half-finished release is never what anyone gets. If a build fails, nothing
is published at all.

## The `playlibre` command

Each installer makes `playlibre` in a terminal open the app: the Windows
installer adds the app's folder to the user's PATH, the `.deb` puts it in
`/usr/bin`, and the AppImage links itself into `~/.local/bin` the first time
it opens (adding that folder to PATH in `~/.bashrc`, `~/.zshrc` or
`~/.profile` if needed). A Mac app
dragged to Applications links itself when it first opens (into
`/usr/local/bin` if that's writable, otherwise `~/.local/bin`), and **More
→ Add the playlibre command** on the home screen puts it in `/usr/local/bin`
with the Mac's password.

## The icon

`packaging/icons/playlibre.svg` is the app icon. The PNGs beside it
(`playlibre-<size>.png`) and `playlibre.ico` are made from it; after changing
it, render it at 1024 pixels and scale those down again. The `.ico` is built
into `playlibre.exe` (`crates/playlibre/build.rs`) and used by the Windows
setup wizard and its shortcuts; the Mac release makes `playlibre.icns` from
the PNGs; the AppImage and the `.deb` use the PNGs for the app menu. The Mac
disk image's window is `packaging/macos/dmg-background.svg` (with its PNGs),
and the Windows wizard's side pictures are `packaging/windows/wizard-*.bmp`.
