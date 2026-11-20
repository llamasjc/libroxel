# Running the official Hub

Libroxel works best with **one Hub everyone uses**: one place for accounts,
friends, published games, the catalog and server lists. Whoever runs that Hub
owns it. A fork runs its own and points its copies of the app at it.

Libroxel's official Hub is `https://hub.libroxel.com` (`OFFICIAL_HUB` in
`crates/libroxel-hub/src/lib.rs`). An app built with an empty
`LIBROXEL_OFFICIAL_HUB=` runs a small Hub of its own instead.

## 1. A server and a domain

Any small Ubuntu server works (1 CPU and 1 GB of memory is plenty to start).
Point a name at it with an `A` record, such as `hub.libroxel.com` →
the server's IPv4 address.

## 2. Set it up with one script

Each GitHub release carries the Hub for servers (`libroxel-hub-linux-x86_64`)
and the two scripts that set it up and update it. On the server:

```sh
curl -fsSLO https://github.com/<owner>/<repo>/releases/latest/download/setup-hub.sh
curl -fsSLO https://github.com/<owner>/<repo>/releases/latest/download/update-hub.sh
sudo bash setup-hub.sh --repo <owner>/<repo> --domain hub.libroxel.com
```

(For a private repository, add `--token <a token that can read it>`, and
copy the two scripts over with `scp` instead.)

The script installs the Hub as a service that starts on boot and restarts
whenever it stops, puts Caddy in front of it for HTTPS (each site has its own
file in `/etc/caddy/sites`, so a website can sit beside it), opens only SSH,
HTTP and HTTPS in the firewall, and backs the database up every night (the
last 14 are kept in `/var/lib/libroxel/backups`).

### Updates

Every 10 minutes the server looks for a new release. When there is one, it
checks each file against the release's `SHA256SUMS.txt`, backs the database
up (`backups/before-<tag>.sqlite`), sets itself up again with that release's
own `setup-hub.sh`, and checks the Hub answers. If it doesn't within 30
seconds, the old Hub goes back and that release is skipped. Publishing a
release (see [RELEASING.md](RELEASING.md)) is all it takes.

- Update now: `sudo libroxel-hub-update --now`
- What happened: `journalctl -u libroxel-hub-update -n 30`
- Settings (domain, repository, token): `/etc/libroxel/hub.conf`

## 3. HTTPS and who is calling

Players' passwords never leave their computers, and every change is signed,
but HTTPS keeps what they browse private and stops anyone in between from
changing lists. The Hub runs with `--behind-proxy`: it listens only on the
server itself, and believes Caddy about who each player is, so rate limits
count each player on their own. Web pages may call the Hub too (it answers
CORS), so a website can show its lists.

## 4. Point the app at it

Set the address in `crates/libroxel-hub/src/lib.rs` (Libroxel's is
`https://hub.libroxel.com`):

```rust
    None => Some("https://hub.libroxel.com"),
```

(or build with `LIBROXEL_OFFICIAL_HUB=https://hub.example.com`). Every copy
built from then on uses it: Online, accounts, friends, publishing and
`libroxel server`. Players can still type another Hub into Online, and
`LIBROXEL_HUB` still picks another one, for testing or a LAN party.

## 5. Give your account admin powers

Make an account in the app like anyone else, with any name you like. Then, on
the Hub's computer, make it an owner:

```sh
sudo libroxel-admin owner add <your name>
```

That's all it takes. Owners are the Hub's admins: their page says so, servers
they host show an **Admin** badge, and only their servers list as
**Official**. Nothing in Libroxel is tied to any particular name.

If you'd like, owners can also take names kept back from players, such as
`admin`, on the Avatar tab (or `sudo libroxel-admin rename <your name> admin`). It's
optional.

To run an official game server, start it with
`libroxel server game.lrx --official`. It prints its id. Add that id as an
owner too.

## Looking after it

All of these run on the Hub's computer, straight on its database, so no one
can do them over the network:

| Command | What it does |
| --- | --- |
| `libroxel hub accounts [search]` | Lists accounts, with owners, passwords and bans |
| `libroxel hub owners` | Lists the owners |
| `libroxel hub owner add\|remove <name or id>` | Adds or removes an owner |
| `libroxel hub ban <name> [reason]` | Bans an account: it can't sign in, host, publish, share, report or befriend anyone, and its page, games and hats stop being listed |
| `libroxel hub unban <name>` | Lets it back |
| `libroxel hub verify\|unverify <name>` | Gives or takes away a verified badge |
| `libroxel hub rename <name> <new name>` | Renames an account, even to a reserved name |
| `libroxel hub reports` | Lists reported games and hats |
| `libroxel hub clear <game\|item> <id>` | Lists a reported one again |
| `libroxel hub remove <game\|item> <id>` | Takes one down for good |

On a server set up with the script, run these as `sudo libroxel-admin …`
(for example `sudo libroxel-admin owner add <your name>`); elsewhere add
`--data <file>` to say where the Hub keeps its data.
