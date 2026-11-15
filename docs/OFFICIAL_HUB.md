# Running the official Hub

Libroxel works best with **one Hub everyone uses**: one place for accounts,
friends, published games, the catalog and server lists. Whoever runs that Hub
owns it. A fork runs its own and points its copies of the app at it.

Until the app has an official Hub, each copy runs a small Hub of its own
(see `OFFICIAL_HUB` in `crates/libroxel-hub/src/lib.rs`).

## 1. A computer that stays on

Any small Linux server works (1 CPU and 1 GB of memory is plenty to start). Give it
a domain name, such as `hub.example.com`, pointing at its address.

## 2. Run the Hub

Build Libroxel on the server (`cargo build --release -p libroxel`) or copy a
release build there. Then run the Hub as a service, so it starts again after a
restart. For example, `/etc/systemd/system/libroxel-hub.service`:

```ini
[Unit]
Description=Libroxel Hub
After=network-online.target

[Service]
User=libroxel
ExecStart=/opt/libroxel/libroxel hub --port 7430 --data /var/lib/libroxel/hub.sqlite --behind-proxy
Restart=always

[Install]
WantedBy=multi-user.target
```

```sh
sudo useradd --system libroxel
sudo mkdir -p /var/lib/libroxel && sudo chown libroxel /var/lib/libroxel
sudo systemctl enable --now libroxel-hub
```

Everything the Hub keeps is in that one SQLite file. Back it up (copy it
while the Hub is stopped, or use `sqlite3 hub.sqlite ".backup copy.sqlite"`).

## 3. HTTPS in front of it

Players' passwords never leave their computers, and every change is signed,
but HTTPS keeps what they browse private and stops anyone in between from
changing lists. [Caddy](https://caddyserver.com) gets a certificate on its
own. `/etc/caddy/Caddyfile`:

```
hub.example.com {
    reverse_proxy 127.0.0.1:7430
}
```

Close port 7430 to the outside, so players can only come in through Caddy.
`--behind-proxy` tells the Hub to believe Caddy about who each player is, so
rate limits count each player on their own. The Hub only believes a web
server on the same computer.

## 4. Point the app at it

Set the address in `crates/libroxel-hub/src/lib.rs`:

```rust
pub const OFFICIAL_HUB: Option<&str> = Some("https://hub.example.com");
```

(or build with `LIBROXEL_OFFICIAL_HUB=https://hub.example.com`). Every copy
built from then on uses it: Online, accounts, friends, publishing and
`libroxel server`. Players can still type another Hub into Online, and
`LIBROXEL_HUB` still picks another one, for testing or a LAN party.

## 5. Give your account admin powers

Make an account in the app like anyone else, with any name you like. Then, on
the Hub's computer, make it an owner:

```sh
libroxel hub --data /var/lib/libroxel/hub.sqlite owner add <your name>
```

That's all it takes. Owners are the Hub's admins: their page says so, servers
they host show an **Admin** badge, and only their servers list as
**Official**. Nothing in Libroxel is tied to any particular name.

If you'd like, owners can also take names kept back from players, such as
`admin`, on the Avatar tab (or `libroxel hub rename <your name> admin`). It's
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

Add `--data /var/lib/libroxel/hub.sqlite` to each command if the Hub keeps its
data there.
