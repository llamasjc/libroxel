# The PlayLibre website

`web/` is the website: the app's home screen in a browser. It shows the same
things from the same Hub as the app:

- **Home**: your friends along the top (who is online, and in which game),
  then every game with how many play it.
- **A game's page**: its servers, with players, who hosts them and which
  friends are in them. **Play** joins the best server (one with friends,
  then the busiest with room) or starts a new one; **Join** picks a server.
- **Friends**: requests, finding people by name, unfriending, and Join to
  go where a friend is playing.
- **Avatar**: colours, hat and a few words about yourself, saved to your
  account so every game shows it.
- **Sign in** with the name and password from the app (and the two-step
  code, if the account uses one), or **make an account** that then works in
  the app too.
- **Download**, for players without the app yet.

It is plain HTML, CSS and JavaScript, with nothing to build. The browser
talks to the Hub directly (the Hub allows that with CORS), so the server
only hands out files.

## Signing in, safely

The website signs in the way the app does. The password is stretched with
Argon2id in the browser into two keys: one proves the password to the Hub,
the other opens the account's sealed identity key. Neither the password nor
the identity key ever goes to the Hub or to the website's server. The
identity key stays in that browser (until the tab closes, or until **Sign
out** if "Keep me signed in" is ticked) and signs every change, just like
the app.

`web/vendor/crypto.js` holds the cryptography: Ed25519, ChaCha20-Poly1305
and SHA-256 from the audited `@noble` libraries and Argon2id from
`hash-wasm`. It is built from `web/vendor/crypto-source.js`:

```sh
npm i @noble/ed25519@2.3.0 @noble/hashes@1.8.0 @noble/ciphers@1.3.0 hash-wasm@4.12.0 esbuild@0.25.0
npx esbuild web/vendor/crypto-source.js --bundle --format=esm --minify --legal-comments=eof --outfile=web/vendor/crypto.js
```

The server sends a Content-Security-Policy that lets only the site's own
scripts run and only lets them talk to the Hub. Pictures may come from the
site itself or the Hub, which draws avatar pictures.

## Play opens the app

Play and Join open a `playlibre://` link:

| Link | What the app does |
| --- | --- |
| `playlibre://join/<join code>` | Joins that server |
| `playlibre://play/<game id>` | Opens the game's page, then joins its best server or starts one |

Game ids are `starter:<name>` for the starter games, or a published game's
id; the part after the action is URL-encoded. The app registers itself for
these links each time it starts (on Linux in
`~/.local/share/applications/playlibre-links.desktop`, on Windows in
`HKEY_CURRENT_USER\Software\Classes\playlibre`; on macOS `Info.plist` does it),
so opening a copy once is enough. If nothing opens, the website offers the
download.

On macOS the link opens PlayLibre but doesn't yet go straight into the game:
macOS hands the link over in a way the app doesn't read yet, so press Play
again in the app. If the app is already open, a link opens a second copy.

## Putting it online

It runs on the same server as the official Hub (see `OFFICIAL_HUB.md`),
with Caddy serving both. The Hub's setup makes `/etc/caddy/Caddyfile` read
every file in `/etc/caddy/sites/`; the website adds `sites/site.caddy` beside
the Hub's `hub.caddy` and leaves the rest alone.

1. **Point the domain at the server.** At your domain's DNS, add two `A`
   records with the server's IPv4 address: one for `@` (the domain itself)
   and one for `www`.
2. **Push the website** to the repository's `main` on GitHub.
3. **Set it up** on the server:

   ```sh
   curl -fsSLO https://raw.githubusercontent.com/<owner>/<repo>/main/packaging/web/setup-site.sh
   sudo bash setup-site.sh --repo <owner>/<repo> --domain playlibre.net --hub https://hub.playlibre.net
   ```

   It installs Caddy and git if they aren't there, fetches `web/` from the
   repository's `main`, copies it to `/var/www/playlibre`, writes
   `/etc/caddy/sites/site.caddy` (with `www` sending people on to the domain
   itself), and reloads Caddy, which gets the HTTPS certificates by itself.

From then on it **updates itself**: every 10 minutes a timer puts online the
same version the Hub on that server runs (its release, or its commit when it
builds from `main`), with that version's own `setup-site.sh`, so the site never
asks the Hub for something it doesn't have. Without a Hub there, it follows the
latest release. To update at once: `sudo playlibre-site-update --now`. The
settings are in `/etc/playlibre/site.conf`; run `setup-site.sh` again with new
options to change them. `journalctl -u playlibre-site-update` shows what the
updates did.

**Downloads** come from the repository's newest GitHub release: the Download
page asks GitHub for it and links each platform's installer
(`…-windows-x86_64-setup.exe`, `…-macos-universal.dmg`, `…-linux-x86_64.AppImage`,
and `…-linux-x86_64.deb` beside it). Before the first release it
says there's nothing to download yet. Without `--repo`, it links to
`/var/www/playlibre/downloads/playlibre-windows-x86_64-setup.exe` and so on instead,
for files you put there yourself.

Without GitHub, put a `web/` folder next to `setup-site.sh` and run it without
`--repo`: that copy goes online as it is, and doesn't update itself.

## Trying it on your computer

Run a Hub and serve `web/` with any static file server, then open the site
with `?hub=` to point it at your Hub (it is remembered in that browser; `?hub=`
on its own forgets it):

```sh
cargo run -p playlibre-hub -- --port 7430 &
python3 -m http.server 8080 --directory web
# open http://127.0.0.1:8080/?hub=http://127.0.0.1:7430
```

The online site's security policy only lets it talk to its own Hub, so
`?hub=` only works on a copy like this.

## Old domains

`setup-site.sh --old-domains "libroxel.com"` sends every page of those
domains (and their `www.`) on to the same page on `--domain`, for good
(HTTP 301). A site set up before the rename to PlayLibre gets this by itself
once `playlibre.net` points at the server; until then it stays on
libroxel.com. The old domains need to point at the server too, so Caddy can
get their certificates.
