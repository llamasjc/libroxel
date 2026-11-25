# Running the official Hub

Libroxel works best with **one Hub everyone uses**: one place for accounts,
friends, published games, the catalog and server lists. Whoever runs that Hub
owns it. A fork runs its own and points its copies of the app at it.

Libroxel's official Hub is `https://hub.playlibre.net` (`OFFICIAL_HUB` in
`crates/libroxel-hub/src/lib.rs`). An app built with an empty
`LIBROXEL_OFFICIAL_HUB=` runs a small Hub of its own instead.

## 1. A server and a domain

Any small Ubuntu server works (1 CPU and 1 GB of memory is plenty to start).
Point a name at it with an `A` record, such as `hub.playlibre.net` →
the server's IPv4 address.

## 2. Set it up with one script

The server builds the Hub itself from the repository's `main` branch, so a
change only has to be pushed, with no release to wait for. On the server:

```sh
git clone https://github.com/<owner>/<repo>.git ~/libroxel
sudo bash ~/libroxel/packaging/hub/setup-hub.sh --repo <owner>/<repo> --domain hub.playlibre.net --from-source
```

The first build takes a few minutes (it installs Rust for your user the
first time). The script installs the Hub as a service that starts on boot
and restarts whenever it stops, puts Caddy in front of it for HTTPS (each
site has its own file in `/etc/caddy/sites`, so a website can sit beside
it), opens only SSH, HTTP and HTTPS in the firewall, and backs the database
up every night (the last 14 are kept in `/var/lib/libroxel/backups`).

For a private repository, add `--token <a token that can read it>`.

### Updates

Every 10 minutes the server looks at `main` on GitHub. When it has changed,
it builds it in `~/libroxel`, backs the database up
(`backups/before-main-<commit>.sqlite`), sets itself up again with that
version's own `setup-hub.sh`, and checks the Hub answers. If it doesn't
within 30 seconds, the old Hub goes back and that version is tried again an hour later.
`~/libroxel` always matches `main`, so don't keep changes of your own there.

- Update now: `sudo libroxel-hub-update --now`
- What happened: `journalctl -u libroxel-hub-update -n 30`
- Settings (domain, repository, token, how it updates): `/etc/libroxel/hub.conf`

#### Or from GitHub releases

To install only releases instead (built by GitHub, checked against each
release's `SHA256SUMS.txt`, nothing to build on the server; see
[RELEASING.md](RELEASING.md)), run `sudo libroxel-hub-update --from-releases`.
`sudo libroxel-hub-update --from-source` switches back. A fresh server can
also start from a release:

```sh
curl -fsSLO https://github.com/<owner>/<repo>/releases/latest/download/setup-hub.sh
curl -fsSLO https://github.com/<owner>/<repo>/releases/latest/download/update-hub.sh
sudo bash setup-hub.sh --repo <owner>/<repo> --domain hub.playlibre.net
```

### Email codes

New accounts give an email, and the Hub mails them a 6-digit code to prove
it's theirs (older accounts are asked to add one). Emails are private: only
the account's owner sees theirs. The Hub sends mail through any SMTP
provider; [Resend](https://resend.com) is free for 3,000 mails a month:

1. Make a Resend account, open **Domains → Add domain** and add
   `playlibre.net`.
2. Add the DNS records it shows at your domain's DNS provider (an `MX` and
   a `TXT` for SPF on `send.playlibre.net`, and a `TXT` for DKIM on
   `resend._domainkey.playlibre.net`). Adding a DMARC record too helps mail
   land in inboxes: `TXT` on `_dmarc.playlibre.net` with
   `v=DMARC1; p=none;`. Wait until Resend says the domain is verified.
3. Open **API Keys → Create API key** (sending access is enough).
4. On the server, put the key in `/etc/libroxel/mail.conf`:

   ```sh
   sudo tee /etc/libroxel/mail.conf >/dev/null <<'EOF'
   LIBROXEL_SMTP_HOST=smtp.resend.com
   LIBROXEL_SMTP_PORT=587
   LIBROXEL_SMTP_USER=resend
   LIBROXEL_SMTP_PASSWORD=re_your_key_here
   LIBROXEL_MAIL_FROM=PlayLibre <no-reply@playlibre.net>
   EOF
   sudo chmod 600 /etc/libroxel/mail.conf
   sudo systemctl restart libroxel-hub
   journalctl -u libroxel-hub -n 5
   ```

   The log says `Mailing email codes through smtp.resend.com:587`. Any
   other provider works the same way with its own SMTP settings. Until mail
   is set up, the Hub keeps the emails people give but sends no codes, and
   doesn't insist on one.

### Messages

Friends can message each other. Bad words are masked on the Hub. To also
check every message with OpenAI's free moderation API, add
`LIBROXEL_MODERATION_KEY=sk-...` to `/etc/libroxel/mail.conf` and restart
the Hub. Reported messages show in `libroxel-hub reports`;
`libroxel-hub remove message <id>` deletes one.

### Game servers

The Hub runs the games players join, on its own computer. Pressing **Play**
on a game asks the Hub for a server: it puts the player on the fullest of
its servers for that game that still has room, or starts a new one
(`libroxel server --game <id>`, the app with no window) and the app waits a
few seconds for it. When a server fills up the Hub starts the next one early,
and each server stops on its own once nobody has been in it for two minutes.
Players join friends' servers first, and anyone can still press **Host it
myself** to run a server on their own computer.

The setup script turns this on: it installs the app as
`/usr/local/bin/libroxel` (built from source, or unpacked from the release),
and opens UDP ports 7500 and up so players connect straight to the server
instead of through a relay. The settings are in `/etc/libroxel/hub.conf`:

| Setting | What it does |
| --- | --- |
| `GAME_SERVERS` | `yes`, or `no` to leave hosting to players |
| `MAX_GAME_SERVERS` | the most at once (default 8); size it to the server's memory |
| `PLAYERS_PER_SERVER` | players on each one (default 12) |
| `FIRST_UDP_PORT` | the first UDP port (default 7500; two per server are opened) |

Change them, then `sudo libroxel-hub-update --now` (or run
`setup-hub.sh` again). Updating the Hub doesn't throw anyone out of a game:
running servers carry on, and new ones use the new version. Each server's
log is in `/var/lib/libroxel/game-servers/<game>.log`; `journalctl -u
libroxel-hub` says when servers start and stop. If your provider has a
firewall of its own, open the same UDP ports there too.

## 3. HTTPS and who is calling

Players' passwords never leave their computers, and every change is signed,
but HTTPS keeps what they browse private and stops anyone in between from
changing lists. The Hub runs with `--behind-proxy`: it listens only on the
server itself, and believes Caddy about who each player is, so rate limits
count each player on their own. Web pages may call the Hub too (it answers
CORS), so a website can show its lists.

## 4. Point the app at it

Set the address in `crates/libroxel-hub/src/lib.rs` (Libroxel's is
`https://hub.playlibre.net`):

```rust
    None => Some("https://hub.playlibre.net"),
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

Names are first come, first served, whatever the case, so make your
account (for example `Admin`) before you open the Hub to players. Accounts
are numbered in the order they joined: the first one is 0, the next 1, and
so on, and profile links use the number (`/accounts/0` on the Hub,
`playlibre.net/#/user/0` on the website). To start the numbers over on a Hub
nobody uses yet, stop it, delete its database and start it again:

```sh
sudo systemctl stop libroxel-hub
sudo rm /var/lib/libroxel/hub.sqlite
sudo systemctl start libroxel-hub
```

To run an official game server, start it with
`libroxel server game.lrx --official`. It prints its id. Add that id as an
owner too.

## Looking after it

All of these run on the Hub's server with the `libroxel-hub` program,
straight on its database, so no one can do them over the network (the
Libroxel app itself can't run or look after a Hub):

| Command | What it does |
| --- | --- |
| `libroxel-hub accounts [search]` | Lists accounts, with owners, passwords and bans |
| `libroxel-hub owners` | Lists the owners |
| `libroxel-hub owner add\|remove <name or id>` | Adds or removes an owner |
| `libroxel-hub ban <name> [reason]` | Bans an account: it can't sign in, host, publish, share, report or befriend anyone, and its page, games and hats stop being listed |
| `libroxel-hub unban <name>` | Lets it back |
| `libroxel-hub verify\|unverify <name>` | Gives or takes away a verified badge |
| `libroxel-hub rename <name> <new name>` | Renames an account |
| `libroxel-hub reports` | Lists reported games and hats |
| `libroxel-hub clear <game\|item> <id>` | Lists a reported one again |
| `libroxel-hub remove <game\|item> <id>` | Takes one down for good |

On a server set up with the script, run these as `sudo libroxel-admin …`
(for example `sudo libroxel-admin owner add <your name>`); elsewhere add
`--data <file>` to say where the Hub keeps its data.

Emails, two-step secrets and messages are encrypted with the key in
`hub.key` beside the data file. Keep it with your backups: without it they
can't be read.
