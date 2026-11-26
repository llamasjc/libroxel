#!/usr/bin/env bash
# Sets up (or updates) the official PlayLibre Hub on an Ubuntu server:
# the Hub as a service that always runs, HTTPS with Caddy, a firewall,
# nightly backups, and updates from GitHub releases.
#
#   sudo bash setup-hub.sh --repo <owner>/<repo> [--domain hub.playlibre.net] [--also "other.names"] [--from-source]
#
# With a playlibre-hub (or playlibre-hub-linux-x86_64) next to this script it
# installs that one. Otherwise, with --from-source it builds main on this
# server (and keeps doing so whenever main changes), or else it downloads
# the latest release. Run it again
# any time: accounts and everything else stay. The domain must already point
# at this server, so Caddy can get its HTTPS certificate.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ "$(id -u)" = 0 ] || { echo "Run it with sudo: sudo bash $0 $*"; exit 1; }

# PlayLibre used to be called Libroxel. A Hub set up back then moves to the new
# names once: settings, data, programs, the service account and the
# services. The old folders stay as links to the new ones, so an updater
# from before the rename that is running this script keeps working.
moved() { # old new: move a folder, leaving a link at the old place
    if [ -e "$1" ] && [ ! -L "$1" ] && [ ! -e "$2" ]; then
        mv "$1" "$2" && ln -s "$2" "$1"
    fi
}
resolves() { getent ahosts "$1" >/dev/null 2>&1; }
if [ -e /etc/libroxel/hub.conf ] && [ ! -L /etc/libroxel ] && [ ! -e /etc/playlibre/hub.conf ]; then
    echo "== Moving the Hub from its old name (Libroxel) to PlayLibre"
    systemctl disable --now libroxel-hub-update.timer libroxel-hub-backup.timer >/dev/null 2>&1 || true
    systemctl disable --now libroxel-hub >/dev/null 2>&1 || true
    # Game servers outlive the Hub's restarts (KillMode=process): stop them
    # too, once, so the account they run as can take its new name.
    if pgrep -u libroxel >/dev/null 2>&1; then
        pkill -TERM -u libroxel || true
        for _ in $(seq 1 10); do pgrep -u libroxel >/dev/null || break; sleep 1; done
        pkill -KILL -u libroxel 2>/dev/null || true
        sleep 1
    fi
    rm -f /etc/systemd/system/libroxel-hub.service /etc/systemd/system/libroxel-hub-backup.service \
        /etc/systemd/system/libroxel-hub-backup.timer /etc/systemd/system/libroxel-hub-update.service \
        /etc/systemd/system/libroxel-hub-update.timer
    systemctl daemon-reload
    if [ -d /etc/playlibre ]; then
        # The website's settings got there first: bring the Hub's beside them.
        for f in /etc/libroxel/*; do [ -e "/etc/playlibre/${f##*/}" ] || mv "$f" /etc/playlibre/; done
        find /etc/libroxel -maxdepth 1 -type l -delete
        rmdir /etc/libroxel 2>/dev/null && ln -s /etc/playlibre /etc/libroxel
    else
        moved /etc/libroxel /etc/playlibre
    fi
    if id libroxel >/dev/null 2>&1 && ! id playlibre >/dev/null 2>&1; then
        usermod -l playlibre -d /var/lib/playlibre libroxel
        getent group libroxel >/dev/null && groupmod -n playlibre libroxel
    fi
    moved /var/lib/libroxel /var/lib/playlibre
    moved /var/lib/libroxel-update /var/lib/playlibre-update
    [ -f /var/lib/playlibre-update/libroxel-hub.previous ] \
        && mv /var/lib/playlibre-update/libroxel-hub.previous /var/lib/playlibre-update/playlibre-hub.previous
    for name in libroxel-hub libroxel-hub.previous libroxel; do
        [ -f "/usr/local/bin/$name" ] && [ ! -L "/usr/local/bin/$name" ] \
            && mv "/usr/local/bin/$name" "/usr/local/bin/${name/libroxel/playlibre}"
    done
    # The old commands go on working, now as the new ones.
    rm -f /usr/local/bin/libroxel-admin /usr/local/bin/libroxel-hub-update
    ln -s playlibre-admin /usr/local/bin/libroxel-admin
    ln -s playlibre-hub-update /usr/local/bin/libroxel-hub-update
    # Settings: the mail ones are read under their new names (the Hub reads
    # the old ones too), the Hub answers on the new domain as well as the
    # old one, which every copy of the app from before the rename uses, and
    # the repository is the renamed one (GitHub sends the old name on).
    [ -f /etc/playlibre/mail.conf ] && sed -i 's/^\(#\{0,1\}\)LIBROXEL_/\1PLAYLIBRE_/' /etc/playlibre/mail.conf
    if grep -q '^DOMAIN="hub.libroxel.com"' /etc/playlibre/hub.conf; then
        if resolves hub.playlibre.net; then
            sed -i 's/^DOMAIN=.*/DOMAIN="hub.playlibre.net"/' /etc/playlibre/hub.conf
            echo 'ALSO_DOMAINS="hub.libroxel.com"' >> /etc/playlibre/hub.conf
        else
            echo "   hub.playlibre.net doesn't point anywhere yet: staying on hub.libroxel.com for now."
            echo 'ALSO_DOMAINS="hub.playlibre.net"' >> /etc/playlibre/hub.conf
        fi
    fi
    sed -i 's#^REPO="llamasjc/libroxel"#REPO="llamasjc/playlibre"#' /etc/playlibre/hub.conf
fi

conf=/etc/playlibre/hub.conf
# What an earlier run chose, unless told otherwise now.
DOMAIN=hub.playlibre.net
# More names the Hub answers to, such as the one it had before (space between them).
ALSO_DOMAINS=""
REPO=""
GITHUB_TOKEN=""
UPDATE_FROM=releases
SOURCE_USER=""
SOURCE_DIR=""
# Game servers on this computer: Play on a game puts players here.
GAME_SERVERS=yes
MAX_GAME_SERVERS=8
PLAYERS_PER_SERVER=12
FIRST_UDP_PORT=7500
[ -f "$conf" ] && . "$conf"
check=yes
while [ $# -gt 0 ]; do
    case "$1" in
        --domain) DOMAIN="$2"; shift 2 ;;
        --also) ALSO_DOMAINS="$2"; shift 2 ;;
        --repo) REPO="$2"; shift 2 ;;
        --token) GITHUB_TOKEN="$2"; shift 2 ;;
        --no-check) check=no; shift ;;
        --from-source) UPDATE_FROM=source; shift ;;
        --no-game-servers) GAME_SERVERS=no; shift ;;
        --game-servers) GAME_SERVERS=yes; shift ;;
        --max-game-servers) MAX_GAME_SERVERS="$2"; shift 2 ;;
        --players-per-server) PLAYERS_PER_SERVER="$2"; shift 2 ;;
        -*) echo "unknown option $1"; exit 1 ;;
        *) DOMAIN="$1"; shift ;;
    esac
done
echo "Setting up the PlayLibre Hub at https://$DOMAIN"

echo "== Packages (Caddy for HTTPS, sqlite3 for backups, ufw firewall, jq for updates)"
if ! command -v caddy >/dev/null || ! command -v sqlite3 >/dev/null || ! command -v jq >/dev/null \
    || ! command -v ufw >/dev/null; then
    apt-get update -q
    DEBIAN_FRONTEND=noninteractive apt-get install -y -q caddy sqlite3 ufw curl jq
fi

echo "== Settings in $conf"
install -d -m 755 /etc/playlibre
umask 077
cat > "$conf" <<CONF
# Read by setup-hub.sh and the updater. Edit, then run: sudo playlibre-hub-update --now
DOMAIN="$DOMAIN"
# More names it answers to (space between them), such as hub.libroxel.com,
# which copies of the app from before the rename to PlayLibre still use.
ALSO_DOMAINS="$ALSO_DOMAINS"
# The GitHub repository whose releases the Hub updates from (owner/name).
REPO="$REPO"
# Only for a private repository: a token that can read it.
GITHUB_TOKEN="$GITHUB_TOKEN"
# How updates come: "releases" (GitHub releases) or "source" (build main on
# this server, as SOURCE_USER in SOURCE_DIR). Switch with
# sudo playlibre-hub-update --from-source or --from-releases.
UPDATE_FROM="$UPDATE_FROM"
SOURCE_USER="$SOURCE_USER"
SOURCE_DIR="$SOURCE_DIR"
# Game servers the Hub runs on this computer ("yes" or "no"): pressing Play
# finds one with room or starts one. At most MAX_GAME_SERVERS at once, with
# PLAYERS_PER_SERVER each, on UDP ports from FIRST_UDP_PORT up (opened in
# the firewall). Each stops once empty for two minutes.
GAME_SERVERS="$GAME_SERVERS"
MAX_GAME_SERVERS="$MAX_GAME_SERVERS"
PLAYERS_PER_SERVER="$PLAYERS_PER_SERVER"
FIRST_UDP_PORT="$FIRST_UDP_PORT"
CONF
umask 022

echo "== The Hub program"
binary=""
# (An updater from before the rename fetches it as libroxel-hub-linux-x86_64.)
for name in playlibre-hub playlibre-hub-linux-x86_64 libroxel-hub-linux-x86_64; do
    [ -f "$here/$name" ] && binary="$here/$name" && break
done
if [ -z "$binary" ]; then
    [ -n "$REPO" ] || { echo "Put playlibre-hub next to this script, or say --repo <owner>/<repo>."; exit 1; }
    install -m 755 "$here/update-hub.sh" /usr/local/bin/playlibre-hub-update 2>/dev/null \
        || curl -fsSL "https://raw.githubusercontent.com/$REPO/main/packaging/hub/update-hub.sh" \
            -o /usr/local/bin/playlibre-hub-update
    chmod 755 /usr/local/bin/playlibre-hub-update
    if [ "$UPDATE_FROM" = source ]; then
        echo "No playlibre-hub here: building main of $REPO on this server."
        exec /usr/local/bin/playlibre-hub-update --from-source ${SOURCE_DIR:+"$SOURCE_DIR"}
    fi
    echo "No playlibre-hub here: fetching the latest release of $REPO."
    exec /usr/local/bin/playlibre-hub-update --now
fi
chmod +x "$binary"
id playlibre >/dev/null 2>&1 || useradd --system --home-dir /var/lib/playlibre --shell /usr/sbin/nologin playlibre
install -d -o playlibre -g playlibre -m 750 /var/lib/playlibre /var/lib/playlibre/backups
[ -f /usr/local/bin/playlibre-hub ] && cp -p /usr/local/bin/playlibre-hub /usr/local/bin/playlibre-hub.previous
install -m 755 "$binary" /usr/local/bin/playlibre-hub.new
mv -f /usr/local/bin/playlibre-hub.new /usr/local/bin/playlibre-hub
if [ -f "$here/update-hub.sh" ]; then
    install -m 755 "$here/update-hub.sh" /usr/local/bin/playlibre-hub-update
fi

echo "== Game servers"
game_flags=""
# (An updater from before the rename unpacks it as libroxel.)
app=""
for name in playlibre libroxel; do
    [ -f "$here/$name" ] && app="$here/$name" && break
done
if [ -n "$app" ]; then
    install -m 755 "$app" /usr/local/bin/playlibre.new
    mv -f /usr/local/bin/playlibre.new /usr/local/bin/playlibre
fi
if [ -x /usr/local/bin/playlibre ]; then
    # What the app needs to run, even with no screen (sound and input libraries).
    if ! ldconfig -p | grep -q 'libasound.so.2' || ! ldconfig -p | grep -q 'libudev.so.1'; then
        apt-get update -q
        DEBIAN_FRONTEND=noninteractive apt-get install -y -q libudev1 \
            || true
        DEBIAN_FRONTEND=noninteractive apt-get install -y -q libasound2t64 2>/dev/null \
            || DEBIAN_FRONTEND=noninteractive apt-get install -y -q libasound2
    fi
    game_flags="--starters /usr/local/bin/playlibre"
fi
if [ "$GAME_SERVERS" = yes ]; then
    # The game server is the PlayLibre app itself, run as `playlibre server` with no window.
    if [ -x /usr/local/bin/playlibre ]; then
        install -d -o playlibre -g playlibre -m 750 /var/lib/playlibre/game-servers
        game_flags="$game_flags --game-servers /usr/local/bin/playlibre --game-server-dir /var/lib/playlibre/game-servers"
        game_flags="$game_flags --max-game-servers $MAX_GAME_SERVERS --players-per-server $PLAYERS_PER_SERVER"
        game_flags="$game_flags --first-udp-port $FIRST_UDP_PORT"
        echo "Up to $MAX_GAME_SERVERS game servers of $PLAYERS_PER_SERVER players each."
    else
        echo "No playlibre app here yet, so no game servers: Play falls back to players hosting."
        echo "(playlibre-hub-update installs it: from source it builds it, from releases it unpacks it.)"
    fi
else
    echo "Off (GAME_SERVERS=no in $conf): players host every game themselves."
fi

echo "== Mail settings in /etc/playlibre/mail.conf (for email codes)"
if [ ! -f /etc/playlibre/mail.conf ]; then
    install -m 600 /dev/null /etc/playlibre/mail.conf
    cat > /etc/playlibre/mail.conf <<'MAIL'
# How the Hub mails codes that prove players' emails. Fill these in from your
# mail provider (Resend, Brevo, Postmark…), then: sudo systemctl restart playlibre-hub
# Until PLAYLIBRE_SMTP_HOST is set, emails are kept but no codes are sent.
#PLAYLIBRE_SMTP_HOST=smtp.resend.com
#PLAYLIBRE_SMTP_PORT=587
#PLAYLIBRE_SMTP_USER=resend
#PLAYLIBRE_SMTP_PASSWORD=your-api-key
#PLAYLIBRE_MAIL_FROM=PlayLibre <no-reply@playlibre.net>
#PLAYLIBRE_MODERATION_KEY=your-openai-key
MAIL
fi

echo "== The service: starts on boot, restarts if it ever stops"
cat > /etc/systemd/system/playlibre-hub.service <<UNIT
[Unit]
Description=PlayLibre Hub
After=network-online.target
Wants=network-online.target
StartLimitIntervalSec=0

[Service]
User=playlibre
Group=playlibre
EnvironmentFile=-/etc/playlibre/mail.conf
ExecStart=/usr/local/bin/playlibre-hub --port 7430 --data /var/lib/playlibre/hub.sqlite --behind-proxy $game_flags
Environment=HOME=/var/lib/playlibre
Restart=always
RestartSec=2
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=/var/lib/playlibre
# Game servers are the Hub's children. Restarting the Hub (as every update
# does) leaves them running, so nobody is thrown out of a game; each stops
# on its own once empty. If memory runs short, a game server goes first.
KillMode=process
OOMPolicy=continue

[Install]
WantedBy=multi-user.target
UNIT

echo "== Nightly backups (the last 14 are kept in /var/lib/playlibre/backups)"
cat > /etc/systemd/system/playlibre-hub-backup.service <<UNIT
[Unit]
Description=Back up the PlayLibre Hub

[Service]
Type=oneshot
User=playlibre
ExecStart=/bin/sh -c 'sqlite3 /var/lib/playlibre/hub.sqlite ".backup /var/lib/playlibre/backups/hub-\$(date +%%F).sqlite" && find /var/lib/playlibre/backups -name "hub-*.sqlite" -mtime +14 -delete'
UNIT
cat > /etc/systemd/system/playlibre-hub-backup.timer <<UNIT
[Unit]
Description=Back up the PlayLibre Hub every night

[Timer]
OnCalendar=*-*-* 04:00:00
Persistent=true

[Install]
WantedBy=timers.target
UNIT

echo "== Updates: every 10 minutes, look for a new version on GitHub"
cat > /etc/systemd/system/playlibre-hub-update.service <<UNIT
[Unit]
Description=Update the PlayLibre Hub from GitHub
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/playlibre-hub-update
UNIT
cat > /etc/systemd/system/playlibre-hub-update.timer <<UNIT
[Unit]
Description=Look for PlayLibre Hub updates

[Timer]
OnBootSec=2min
OnUnitActiveSec=10min
RandomizedDelaySec=60

[Install]
WantedBy=timers.target
UNIT

echo "== HTTPS with Caddy"
# Each site gets its own file in /etc/caddy/sites, so the website can sit beside the Hub.
install -d /etc/caddy/sites
grep -q 'import /etc/caddy/sites/\*' /etc/caddy/Caddyfile 2>/dev/null \
    || echo 'import /etc/caddy/sites/*.caddy' > /etc/caddy/Caddyfile
names="$DOMAIN"
for also in $ALSO_DOMAINS; do names="$names, $also"; done
cat > /etc/caddy/sites/hub.caddy <<CADDY
$names {
    encode gzip
    header {
        Strict-Transport-Security "max-age=31536000"
        X-Content-Type-Options nosniff
    }
    reverse_proxy 127.0.0.1:7430
}
CADDY

echo "== Firewall: SSH, HTTP and HTTPS, and UDP for game servers"
ufw allow OpenSSH >/dev/null
ufw allow 80/tcp >/dev/null
ufw allow 443/tcp >/dev/null
if [ -n "$game_flags" ]; then
    # Two ports for each server, so a restarted one never waits on its old port.
    last_udp_port=$((FIRST_UDP_PORT + 2 * MAX_GAME_SERVERS - 1))
    ufw allow "$FIRST_UDP_PORT:$last_udp_port/udp" >/dev/null
    echo "Game servers take players on UDP $FIRST_UDP_PORT-$last_udp_port."
fi
ufw --force enable >/dev/null

# Shortcut for looking after it: sudo playlibre-admin accounts, owners, ban…
cat > /usr/local/bin/playlibre-admin <<'ADMIN'
#!/bin/sh
exec sudo -u playlibre /usr/local/bin/playlibre-hub "$@" --data /var/lib/playlibre/hub.sqlite
ADMIN
chmod 755 /usr/local/bin/playlibre-admin

# The official Hub's own admin: the account llamas becomes an owner, once,
# on the first setup or update that has this. Only if that account is
# already there, so no one can make it later to become an owner.
seeded=/var/lib/playlibre/owner-llamas-done
if [ "$DOMAIN" = hub.playlibre.net ] && [ ! -e "$seeded" ] && [ -f /var/lib/playlibre/hub.sqlite ]; then
    if /usr/local/bin/playlibre-admin owner add llamas; then :; else echo "No account llamas here, so no owner was added."; fi
    touch "$seeded"
fi

systemctl daemon-reload
systemctl enable --now playlibre-hub playlibre-hub-backup.timer >/dev/null
if [ -n "$REPO" ] && [ -x /usr/local/bin/playlibre-hub-update ]; then
    systemctl enable --now playlibre-hub-update.timer >/dev/null
fi
systemctl restart playlibre-hub
systemctl enable caddy >/dev/null 2>&1 || true
systemctl reload caddy 2>/dev/null || systemctl restart caddy

[ "$check" = yes ] || exit 0
echo "== Checking it answers (the HTTPS certificate can take a minute the first time)"
for _ in $(seq 1 30); do
    if curl -fsS "https://$DOMAIN/" >/dev/null 2>&1; then
        curl -fsS "https://$DOMAIN/"; echo
        echo "Done: the Hub is live at https://$DOMAIN"
        if [ -n "$REPO" ] && [ "$UPDATE_FROM" = source ]; then
            echo "It builds and installs $REPO's main branch whenever it changes."
        elif [ -n "$REPO" ]; then
            echo "It updates itself from the releases of $REPO."
        fi
        exit 0
    fi
    sleep 4
done
echo "The Hub runs, but https://$DOMAIN doesn't answer yet. Check: sudo journalctl -u caddy -n 50"
