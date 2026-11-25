# Security

PlayLibre runs other people's games, and the Hub keeps accounts, passwords,
email addresses and two-step sign-in secrets, so security problems matter
here. Thank you for reporting them carefully.

## Reporting a problem

**Please don't open a public issue for a security problem.** Report it
privately instead:

1. Open the repository's **Security** tab on GitHub.
2. Choose **Report a vulnerability** and describe what you found.

Please include:

- what an attacker can do, and what they need first (an account, a game
  someone joins, being on the same network, and so on);
- the steps to show it, or a small game file or script that does;
- the version (shown at the bottom right of the home screen and in the
  window title) and your operating system.

You'll get an answer within a week. Once a fix is out, the problem can be
described publicly, with credit to you if you'd like it.

## What counts

- **The app:** a game's scripts getting out of the sandbox, a game or a
  joined player crashing or freezing someone else's computer, reading or
  changing files outside the games folder, or the updater installing
  something that isn't a signed PlayLibre release.
- **Multiplayer:** a joined computer taking control of the host, of other
  players, or of what the host's scripts do in ways a normal player can't.
- **The Hub and the website:** getting into someone else's account, seeing
  private information (emails, friends lists, messages, where friends are),
  getting around rate limits, bans or parental settings, or publishing as
  someone else.

Some things are known and don't need a report:

- Every game and edit-together connection is encrypted, but a direct join
  by address doesn't check who the host is, so someone on the same network
  could pretend to be the host. Both ends show a safety code in the output
  that only matches when nobody is in the middle. Join codes do check: the
  code names the host's key. While a game has a join code, it takes players
  only through it.
- A host runs every server script and sees everything its players send.
  A player's name is what they typed; players who join by code are also
  known to the host by their key, and someone removed that way stays out
  whatever name they use.
- A script can use up to 5 seconds without waiting. A game's scripts may
  use 2 GB of memory together on a computer that hosts or plays on its own,
  768 MB on a phone or tablet, and 512 MB on a computer that joined someone
  else's game.

## Supported versions

Only the latest release gets security fixes. The app updates itself from
GitHub releases, and the official Hub updates itself within about 10
minutes of a release.
