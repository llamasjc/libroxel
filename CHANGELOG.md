# Changelog

What changed in each release. The full list of commits for each one is on
its [GitHub release](https://github.com/llamasjc/libroxel/releases) page.

## Unreleased

- **Voice chat** in multiplayer games, for players 13 and up whose parent
  settings allow chat. The 🎤 button turns the mic on; click a name to mute.
- **Languages:** the home screen and the editor in Chinese (Simplified),
  Spanish, Portuguese (Brazil) and French, picked with the 🌐 button or
  following the computer's language. API names stay in English.
- **Streaming,** as Roblox documents it: `Workspace.StreamingEnabled`,
  `StreamingTargetRadius`, `StreamingMinRadius`, `StreamOutBehavior`,
  `Model.ModelStreamingMode`, `Player.ReplicationFocus` and
  `Player:RequestStreamAroundAsync`. Places saved with them in Roblox
  Studio keep them.
- **Multiplayer:** every connection is encrypted, direct joins by address
  included. Long messages are compressed, so joining a big game sends up to
  ten times less. The host refuses impossible moves (teleports and speed
  hacks), limits how much a joined computer may send, and hangs up on ones
  that never say hello. Two players on one spawn no longer push each other
  up into the sky. Older copies can't play with this one; they're told so.
- **Unsaved work** is copied every minute and offered back after a crash.
- **Screen readers** (Narrator, VoiceOver, Orca) read the menus and panels
  on Windows, macOS and Linux.
- **ARM builds** for Windows on ARM and 64-bit ARM Linux, with their own
  updates.
- **Libroxel's own game art and sounds:** the chess, checkers and Connect 4
  pieces, the cards and table cloth are drawn by Libroxel, and every game
  sound is made in code, replacing the GPL and AGPL ones. Chess has four
  sound sets (wood, glass, retro, digital), and chess and checkers have
  wooden pieces too. Games using the older sound names still work.
- `Model:AddPersistentPlayer` and `RemovePersistentPlayer`.
- The editor's help for each class is translated too.
- The desktop Mac apps build on `macos-15`, where Macs are easier to get.
- Hide and seek's seeker bots search spot by spot instead of at random.
- The website no longer shows "falsefalse" in the top bar.

## 0.1.5 (2026-11-23)

- The Android app opens on 32-bit tablets and phones again. Luau's JIT is
  left out of 32-bit ARM builds, where it never ran anyway.
- Tag releases build their Mac and iOS apps after `main`'s build of the
  same commit, so the two runs don't wait on the same Macs.

## 0.1.4 (2026-11-23)

- **Android and iOS apps**, signed for Google Play and TestFlight, with
  Android copies from GitHub updating themselves. 32-bit ARM Android builds
  were added (these didn't open until 0.1.5).
- **Accounts:** recovery codes, password reset by email and signing out
  everywhere, in the app and on the website. Birthdays, parent approval and
  parent settings, and a strict chat filter for kids.
- **Social:** messages between friends, groups (roles, a wall, group games
  and badges), likes, favorites and comments on games, and a Profile tab.
- **Finding games:** Charts, Continue and Recommended on the home screen,
  and creator analytics on the website's Create tab.
- **Moderation:** a dashboard with warnings, mutes, timed bans and a log.
- **Crashes:** a crash screen and Report buttons on errors. Crash reports
  and usage stats go to the Hub, with a stats page for its owners.
- **Look:** warmer, more vivid lighting, the camera on the head with your
  own body fading in first person, and F11 for fullscreen. The version is
  shown on the home screen and in the window title.
- **Fixes:** eyes and hats stay on other players' heads, prompts work for
  joined players, and big menus no longer blank the screen.

## 0.1.3 (2026-11-22)

- Big games can be published: uploads are compressed, up to 100 MB.
- Battle royale: an airship drop, tiered loot, shields, building and storm
  phases.
- A Games tab, and the avatar drawn from one shared file.
- Riders keep facing their seat.

## 0.1.2 (2026-11-22)

- Parts no longer blink out when they merge, and far shadows are fixed.
- The website stays on the same version as the Hub, with a privacy page,
  clean addresses and a sitemap.

## 0.1.1 (2026-11-22)

- The app updates itself from releases. Windows and Mac builds can be
  signed.
- Join with your account and avatar. The Hub's game list shows in the app
  and on the website, and games are kept on the Hub and fetched on Play.
- Esc opens a game menu instead of leaving.
- Fixes for map flicker, kart stutter, first person, walking, and scripts
  running out of memory in big games.

## 0.1.0 (2026-11-21)

The first release.

- The editor, Luau scripting, physics, and playing together over join
  codes.
- The Hub: accounts with passwords and two-step sign-in, friends,
  publishing, server lists, rate limits and the official Hub at
  hub.libroxel.com.
- The libroxel.com website, where Play opens the app.
- Starter games: an arcade kart race, chess, checkers, Connect 4, Kings in
  the Corner, Tag, Hide and seek and Battle royale.
- Installers for Windows, macOS and Linux.
