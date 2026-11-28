# Changelog

What changed in each release. The full list of commits for each one is on
its [GitHub release](https://github.com/llamasjc/playlibre/releases) page.

## 0.1.26 (2026-11-28)

- **Scripts:** the PlayerModule's controls (`GetControls`, `Enable`,
  `Disable`, `GetMoveVector`) work, and actions bound to `Enum.PlayerActions`
  can hold the character still. `IsWindowFocused`, `SharedTableRegistry`,
  `SetGameplayPausedNotificationEnabled` and the EditableMesh colour and
  normal calls are there too.

## 0.1.25 (2026-11-28)

- **Scripts:** `PlayerGui:GetGuiObjectsAtPosition` and the older
  `PhysicsService` collision group calls work. Local scripts can move their
  own character in joined games, so players no longer fall during cutscenes.
- **Clock:** signing in and other signed requests work when the computer's
  clock is off.
- **Leaving a game** goes back to Studio only when the game was started
  from Studio.
- **Host it myself** is gone.

## 0.1.24 (2026-11-27)

- **Android:** 32-bit phones get the app again; 0.1.23 didn't build for them.

## 0.1.23 (2026-11-27)

- **Limited items:** creators can make a priced item limited, with a stock,
  a number of days on sale, or both. Each copy has a serial number. Owners
  can sell copies on for Credits (30% fee) with a price history, and trade
  them: one player offers, the other accepts or declines, and both confirm
  before the items move.

- **Group games and funds:** a group's owner picks what each member can
  do: edit the group's games, pay from its funds, clear the wall, or manage
  members. Members who can edit a group's games can download, publish,
  and manage them. Sales and play time from group games go to the group's
  funds, which can pay members.

- **Brick body:** a second body to pick on the Avatar page, with a round
  head, hands, feet and hips. Animations, clothes and hats work on it.

- **Developer products:** creators can sell things players buy again and
  again, such as coins, on the Products tab of a game on the Create page.
  `MarketplaceService:PromptProductPurchase` shows the product in the
  player's app, and each purchase goes to `MarketplaceService.ProcessReceipt`
  until it answers `PurchaseGranted`, so a purchase is granted once even if
  the player leaves or the server stops first. Creators get their share as
  with passes.

## 0.1.22 (2026-11-27)

- **No free Credits:** badges no longer give Credits, and a creator's
  share of a sale only counts what the buyer paid for. On the Hub,
  `playlibre-admin credits <name>` shows where an account's Credits came
  from, and `playlibre-admin credits free take` takes back the free ones.

- **Phones:** the home screen has a compact top bar on small screens, with
  Donate and Language in a menu. In games, a menu button and a player list
  button sit at the top left, the player list starts hidden, the pause menu
  has bigger buttons and fits short screens, and buttons and text are
  bigger to tap.

## 0.1.21 (2026-11-26)

- **Loading screen lines:** the turning cubes on the loading screen no
  longer flash thin lines across the screen. When a cube face turned
  almost side-on, the corners of its outline were drawn as long needles;
  faces and their edges are now drawn separately so no corner can stick
  out. The little cubes going round behind the big one are now hidden by
  it instead of showing through.

- **Big games on servers:** a game with more than 5,000 parts no longer
  streams unless it turns `StreamingEnabled` on itself. Before, its
  players only got the parts near them, so scripts that use far parts
  (`workspace.Shop.Counter`) stopped with an error, though the game
  worked in the editor. Every part now comes, as it does there.
- **Faster first join:** a server makes its players' code hard to read
  as it starts, on other threads, instead of while the first player
  joins. In a test game with 300 modules this took the server's longest
  pause from about 1 s to 19 ms. It also remembers that code by size
  rather than by count, so a game with thousands of modules isn't worked
  out again for each player.
- **Saved progress:** games played on the Hub's servers keep their data
  stores on the Hub, so coins, levels and unlocks carry over from one
  server to the next and outlast a server stopping. Servers trade changes
  with the Hub every few seconds and send the last ones when they stop.
  A creator hosting their own game keeps its data there too.
- **Game passes inside games:** `MarketplaceService:PromptGamePassPurchase`
  shows the pass with its price and buys it with the player's Credits,
  `UserOwnsGamePassAsync` answers from the Hub, and `GetProductInfo` with
  `Enum.InfoType.GamePass` gives the pass's name and price.
- **Game icons and screenshots:** creators put up an icon and up to six
  screenshots on their game's Settings page on the website. They show on
  the game's page, in lists and in the app. The Hub draws every picture
  again at a set size before keeping it.
- **Genres and tags:** creators pick their game's genre and up to five
  tags. A game's page links to other games with the same genre or tag,
  and Search finds games by their tags.
- **Following and notifications:** follow a game to hear when it is
  updated, and a creator to hear about their new games. The bell at the
  top of the website lists friend requests, messages, group invites,
  badges, sales and news from what you follow.
- **Games with more than one place:** creators add more places to a game
  (a lobby and its levels, say) on its Settings page. Scripts send
  players between them with `TeleportService` (`Teleport`,
  `TeleportAsync`, `TeleportToPrivateServer`, `TeleportPartyAsync`,
  `ReserveServer`), with teleport data that `Player:GetJoinData()` and
  `GetLocalPlayerTeleportData` give back there. `game.PlaceId`,
  `game.GameId`, `game.JobId`, `game.PrivateServerId` and
  `game.PrivateServerOwnerId` say where a script is. Every place shares
  the game's saved data.
- **Private servers:** make a server of a game for you and your friends
  from its page on the website. Only you and your friends can join it,
  and it isn't on the server list. Reserved servers from `ReserveServer`
  only take the players a script sends there.
- **Saved data follows you:** a player's `UserId` comes from their key, so
  it is the same on every server of a game and their saved data finds
  them.
- **Clothes:** Hat-Makers add shirts (short or long-sleeved) and trousers
  to the catalog from the Avatar page on the website. Players wear them in
  the app and on the website, and characters wearing them have a `Shirt`
  or `Pants` in them.

## 0.1.20 (2026-11-26)

- **Game pages have numbers:** every game has a number, and its page on
  the website is `/games/<number>/<name>` (such as
  `/games/12/my-cool-game`). The number alone opens it, a wrong or old
  name moves to the right address, and old `/game/<id>` links still work.
  Games a Hub already has are numbered when it starts, and a number never
  changes or goes to another game. My games in the app has a Page button
  that opens it.
- **Website:** the site loads again. 0.1.19 stopped before showing any tabs,
  because a part of the page was there twice. The site's update also puts
  its files in place when its folder is a link to the old one.
- **Avatars:** the app shows the look the account has on the Hub, the
  same one the website and other players see. Changes made on the website
  reach a game already being played. Picking a look needs an account.
- **Credits:** there are no more free daily Credits. Your balance shows at
  the top right, by your name, in the app and on the website.
- **Hats:** only Hat-Makers add hats to the catalog. The Hub's owners give
  that out on the website's Moderation page or with
  `playlibre-hub hat-maker add|remove`.

## 0.1.19 (2026-11-26)

- **Hub servers:** the updater sets the Hub up again when its service is
  missing, instead of saying it is already on that version and leaving
  the Hub stopped (which a server moved from Libroxel could hit).
- **Website:** avatar pictures (heads on profiles and friends, whole bodies
  on profiles and the Avatar page) now show. The site's security rules
  only allowed pictures from the site itself, so the browser blocked the
  ones the Hub draws.
- **Android:** menus stay clear of the notch again in release builds.
- **Built-in pictures, sounds and animations are `playlibre://…` now**
  (`playlibre://coin`, `playlibre://wave` and so on) in the editor's help,
  the starter games and the docs. `libroxel://…` names in games made
  before the rename still work the same.
- Script errors no longer name the old `libroxel.` chunks, and the Hub's
  status page says `"hub": "playlibre"`.
- **Moving from Libroxel to PlayLibre without losing anyone:**
  - Macs on a Libroxel version update to PlayLibre by themselves (the update
    archive also holds the app under its old name, which their updater
    looks for).
  - A Hub or website server set up as Libroxel moves itself to the new
    names (settings, data, service account, services and commands) the
    first time the new `setup-hub.sh` or `setup-site.sh` runs. The old
    folders stay as links, and `libroxel-admin` keeps working.
  - The Hub answers on both `hub.playlibre.net` and `hub.libroxel.com`,
    which older copies of the app use, and reads `LIBROXEL_…` settings
    where there is no `PLAYLIBRE_…` one, so email codes keep going out.
  - libroxel.com (and www.libroxel.com) sends every page on to the same
    page on playlibre.net.
- **Games are much harder to steal:**
  - A published game's place, with its server scripts and storage, now
    goes only to its creator and the Hub's own servers, on every Hub.
    Everyone else plays it by joining a server. (Before, on a Hub without
    servers of its own, anyone could download the whole game.)
  - Local scripts and the modules players' computers can see are sent with
    no comments, no indentation and no local names. They run exactly the
    same (checked against the original's bytecode before sending) and
    errors still point at the right line.
  - Game scripts can't read any script's `Source`, so a careless
    RemoteFunction can't hand out the server's code, and a failed
    `InvokeServer` no longer shows the player the server's stack trace.
  - Pictures, meshes and sounds kept from the Hub are sealed on disk with
    this computer's own key, and are checked against their hash when they
    arrive.
  - Joining someone else's game from the editor shows it the way players
    see it: no Explorer, Properties or script editor over their game.

## 0.1.16 (2026-11-24)

- **Android:** the app opens again on 64-bit phones. It quit on launch
  with "cannot locate symbol `__clear_cache`".
- **Steadier multiplayer:**
  - Both sides of a game say something at least once a second, so a
    connection that goes quiet without hanging up (Wi-Fi that drops, a
    computer that sleeps) is noticed after 20 seconds: the host lets that
    player go instead of keeping them standing there for ever with the
    world piling up for them, and the joined computer says the host
    stopped answering instead of freezing.
  - A joined game whose connection drops tries to come back by itself a
    few times (1, 2, 4, 8 and 16 seconds apart, once the host answers),
    and the status bar has a Reconnect button. The player comes back with
    the same name and look and gets the world as it is now.
  - When the host stops the game, everyone still in it is told "The host
    ended the game" (and nothing tries to reconnect).
  - `InvokeServer` no longer waits for ever: it fails when the connection
    is lost, when the RemoteFunction isn't on the server (or is gone from
    it), or when a computer calls too often.
  - Long games no longer keep everything they ever destroyed: bullets,
    effects and old characters are freed every few seconds once no script
    can reach them, on the host and on every joined computer. Fifteen
    seconds of ten bullets a frame used to leave about 90,000 of them in
    memory.
  - Streamed games no longer collect a list of held-back changes that
    only grew while things were being made or destroyed.
  - New tests play games over a pretend network (delay, jitter, lost
    packets, lag spikes, connections that go silent or hang up) with
    many players joining and leaving, and check each joined computer
    ends up with exactly the host's world.

- **Safer for creators and players:**
  - Edit together now needs the share's code or password, and the person
    sharing lets each guest in by name before they see anything. Each
    share uses a key of its own, so its code can't be worked out from
    the sharer's player id.
  - A game can only open picture, sound and model files inside its own
    folders: no full paths elsewhere, no `..`, no network shares (which
    on Windows would hand someone the user's sign-in), only ordinary
    files up to 64 MB. Pictures that claim to be huge are not unpacked.
  - Instances a player's computer names in `FireServer` and
    `InvokeServer` reach the server's scripts only if that computer was
    sent them, as on the reference platform: nothing from `ServerStorage`.
  - Walking through walls is caught like moving too fast.
  - Players who join by code are known to the host by their key: someone
    removed from a game can't come back under another name. While a game
    has a join code it takes players only through it, and joins by
    address show a safety code on both computers.
  - The Hub lists a server under a game only when its host signs it, and
    the join code must reach the computer that signed.
  - The Hub keeps password salts and keys encrypted with its key file,
    gives the same made-up salt for names with no password (so the
    sign-in page can't tell which names have one), and counts wrong
    passwords per name and address, so a stranger can't lock someone out.
  - `HttpService:GenerateGUID` uses the system's secure random numbers.
  - A joined computer's scripts may use 512 MB of memory, and a phone's 768 MB.
- **Faster:** the host writes each step's changes out once for all its
  players, not once per player; players joining in the same step share one
  copy of the world; parts with the same picture and look share one
  material, so they draw together; and their pictures load in the
  background, without stalling the frame. The Hub's database uses WAL.

- **Game services act as documented:** data stores hand back each key's
  `DataStoreKeyInfo` (version, times, user ids, metadata), keep scopes
  apart, keep older versions for `GetVersionAsync` and
  `ListVersionsAsync`, and ordered stores take only whole numbers.
  Bound actions run highest priority first and stop at the first one that
  doesn't pass, and unbinding a held key cancels it. Render steps run in
  priority order. `CollectionService.TagAdded` and `TagRemoved` fire.
  Sounds fire `Played`, `Paused`, `Resumed` and `Stopped`. A newer tween
  cancels an older one moving the same thing, and booleans can be
  tweened. Text chat channels deliver messages (with `OnIncomingMessage`
  tags) and run slash commands. `GetTextSize` wraps at the frame width.
  Purchase prompts close as not bought instead of never answering, and
  badges awarded in a game stay awarded.
- **Avatar pictures:** profile pictures in the app and on the website
  are now 3D renders of your character, head and full body, redrawn
  when your look changes.
- **Imported places keep more:** attributes holding a NumberRange,
  NumberSequence, ColorSequence, Rect or CFrame, particle emitters'
  Lifetime and Speed, and parts' `CustomPhysicalProperties` now come
  across (they were dropped).
- **Materials have physics:** parts weigh, grip and bounce by their
  material as on the reference platform (ice slides, metal is heavy; plastic is as
  before), and `CustomPhysicalProperties` sets a part's own.
- **Fixes:** attributes of the kinds above now save and reach joined
  players (they read nil before); any humanoid whose health reaches 0
  fires `Died`, not only characters; tweens have `PlaybackState`, and
  `Completed` says how they ended.
- **Phones:** menus and buttons stay clear of a notch, sounds and voice
  chat pause while Libroxel is in the background (with unsaved work kept),
  and Back on the home screen leaves the app.
- **Closed games:** on a Hub that runs its own servers, only a game's
  creator and those servers can download its place, so its server scripts
  and server storage never reach players. Copies of someone else's game
  are turned away when published, and bulk downloads of games and files
  are limited and logged.
- **Big maps stream:** each player is sent only the parts and terrain
  near them, nearest first, with far parts updated less often. Maps with
  more than 5,000 parts stream even without `StreamingEnabled`, so no
  player's computer ever gets the whole map.
- **Your games** on the Create page, in the app and on the site: change a
  game's name and description, who can play, and max players, remove it,
  or put an older version back live.
- **More of the scripting API:** joints (`GetJoints`,
  `GetConnectedParts`, `AssemblyRootPart`, `IsGrounded`), `Part:Resize`,
  collision group renames and checks, `WorldModel` raycasts and part
  queries, terrain `CopyRegion`, `PasteRegion`, `ReadVoxels` and
  `ReadVoxelChannels`, and older lowercase names like `findFirstChild`.
- **Android tablets without Vulkan** (Mali and similar) open instead of
  closing on the first frame.
- **Phones and tablets:** the game fills the screen with the status and
  navigation bars hidden (a swipe in from the edge shows them), and the
  screen stays on while playing. The on-screen keyboard types into text
  boxes, and the back button opens and closes the game's menu.
- **Touch controls** are laid out as in phone games: the stick comes up
  wherever the left thumb lands, the stick and jump button grow with the
  screen, and the game's own buttons keep their taps. The loading screen
  has an X to stop waiting.
- **Faster on phones:** automatic graphics starts low and stays in the
  middle, without the effects that cost phones most, and 32-bit Android
  uses the chip's vector maths.
- **Touch input for scripts,** as on the reference platform: `TouchStarted`, `TouchMoved`,
  `TouchEnded`, `TouchTap`, `TouchTapInWorld`, `TouchLongPress`,
  `TouchPan`, `TouchPinch`, `TouchRotate`, `TouchSwipe`,
  `LastInputTypeChanged`, and InputBegan/InputChanged/InputEnded for
  fingers. Games can hide the stick and jump button with `ModalEnabled`,
  `GuiService.TouchControlsEnabled` or `DevTouchMovementMode` Scriptable.
- **More of the reference platform on phones:** `ContextActionService:BindAction` touch
  buttons (with `SetTitle` and `SetPosition`), `HapticService` vibration on
  Android, and `TouchTap` and `TouchLongPress` on GUI objects.
- **Sharper and faster on phones:** the 3D view draws with fewer pixels
  than the screen has while menus and text stay sharp, and Libroxel's logo
  shows on its blue while it starts. On iPhones and iPads the home bar
  fades away while playing and swipes from an edge reach the game first.
- **Voice chat** in multiplayer games, for players 13 and up whose parent
  settings allow chat. The 🎤 button turns the mic on; click a name to mute.
- **Languages:** the home screen and the editor in Chinese (Simplified),
  Spanish, Portuguese (Brazil) and French, picked with the 🌐 button or
  following the computer's language. API names stay in English.
- **Streaming,** as the reference platform documents it: `Workspace.StreamingEnabled`,
  `StreamingTargetRadius`, `StreamingMinRadius`, `StreamOutBehavior`,
  `Model.ModelStreamingMode`, `Player.ReplicationFocus` and
  `Player:RequestStreamAroundAsync`. Places saved with them on the reference platform
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
