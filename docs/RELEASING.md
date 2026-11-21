# Releasing Libroxel

`.github/workflows/release.yml` builds the Libroxel app for every platform
and attaches the builds to a GitHub Release whenever a version tag is pushed.

| File | Platform | What's inside |
| --- | --- | --- |
| `libroxel-<version>-linux-x86_64.tar.gz` | Linux (x86-64, glibc 2.35+) | the `libroxel` executable |
| `libroxel-<version>-windows-x86_64.zip` | Windows 10/11 (x86-64) | `libroxel.exe` |
| `libroxel-<version>-macos-universal.zip` | macOS 11+ (Apple silicon and Intel) | `Libroxel.app` |
| `libroxel-hub-linux-x86_64` | Linux servers (x86-64) | the Hub on its own, for the official Hub server |
| `setup-hub.sh`, `update-hub.sh` | | set a Hub server up, and update it (see [OFFICIAL_HUB.md](OFFICIAL_HUB.md)) |
| `SHA256SUMS.txt` | | checksums of the files above |

Each build is a single file: the built-in pictures and starter games are compiled
into it, so nothing else needs to sit next to it.

## Publishing a release

1. Bump `version` under `[workspace.package]` in `Cargo.toml` to match the
   tag you're about to make (optional), commit, and push to `main`.
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
rebuilds only Libroxel's own code.

To try the builds without publishing anything, open **Actions → Release → Run
workflow**. The files are attached to that run as downloadable artifacts.

If a release goes wrong, delete it and its tag on GitHub, fix the problem, and
push the tag again.

## Running a downloaded build

- **Windows:** unzip and double-click `libroxel.exe`. SmartScreen may warn
  that the app is from an unknown publisher, because it isn't code-signed:
  choose **More info → Run anyway**.
- **macOS:** unzip and open `Libroxel.app`. It is not notarized by Apple, so
  the first time, right-click it and choose **Open**, or run
  `xattr -dr com.apple.quarantine Libroxel.app`.
- **Linux:** `tar xzf libroxel-*.tar.gz` and run `./libroxel`. It needs a
  Vulkan driver, plus Wayland or X11, which desktop installs already have.

## Not done yet

- Code signing for Windows and notarization for macOS (both need paid
  certificates; add them as repository secrets when there are some).
- Linux and Windows builds for ARM.
