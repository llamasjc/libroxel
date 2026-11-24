#!/usr/bin/env bash
# Sets up (or updates) the official Libroxel Hub on an Ubuntu server:
# the Hub as a service that always runs, HTTPS with Caddy, a firewall,
# nightly backups, and updates from GitHub releases.
#
#   sudo bash setup-hub.sh --repo <owner>/<repo> [--domain hub.libroxel.com] [--from-source]
#
# With a libroxel-hub (or libroxel-hub-linux-x86_64) next to this script it
# installs that one. Otherwise, with --from-source it builds main on this
# server (and keeps doing so whenever main changes), or else it downloads
# the latest release. Run it again
# any time: accounts and everything else stay. The domain must already point
# at this server, so Caddy can get its HTTPS certificate.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ "$(id -u)" = 0 ] || { echo "Run it with sudo: sudo bash $0 $*"; exit 1; }

conf=/etc/libroxel/hub.conf
# What an earlier run chose, unless told otherwise now.
DOMAIN=hub.libroxel.com
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
echo "Setting up the Libroxel Hub at https://$DOMAIN"

echo "== Packages (Caddy for HTTPS, sqlite3 for backups, ufw firewall, jq for updates)"
if ! command -v caddy >/dev/null || ! command -v sqlite3 >/dev/null || ! command -v jq >/dev/null \
    || ! command -v ufw >/dev/null; then
    apt-get update -q
    DEBIAN_FRONTEND=noninteractive apt-get install -y -q caddy sqlite3 ufw curl jq
fi

echo "== Settings in $conf"
install -d -m 755 /etc/libroxel
umask 077
cat > "$conf" <<CONF
# Read by setup-hub.sh and the updater. Edit, then run: sudo libroxel-hub-update --now
DOMAIN="$DOMAIN"
# The GitHub repository whose releases the Hub updates from (owner/name).
REPO="$REPO"
# Only for a private repository: a token that can read it.
GITHUB_TOKEN="$GITHUB_TOKEN"
# How updates come: "releases" (GitHub releases) or "source" (build main on
# this server, as SOURCE_USER in SOURCE_DIR). Switch with
# sudo libroxel-hub-update --from-source or --from-releases.
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
for name in libroxel-hub libroxel-hub-linux-x86_64; do
    [ -f "$here/$name" ] && binary="$here/$name" && break
done
if [ -z "$binary" ]; then
    [ -n "$REPO" ] || { echo "Put libroxel-hub next to this script, or say --repo <owner>/<repo>."; exit 1; }
    install -m 755 "$here/update-hub.sh" /usr/local/bin/libroxel-hub-update 2>/dev/null \
        || curl -fsSL "https://raw.githubusercontent.com/$REPO/main/packaging/hub/update-hub.sh" \
            -o /usr/local/bin/libroxel-hub-update
    chmod 755 /usr/local/bin/libroxel-hub-update
    if [ "$UPDATE_FROM" = source ]; then
        echo "No libroxel-hub here: building main of $REPO on this server."
        exec /usr/local/bin/libroxel-hub-update --from-source ${SOURCE_DIR:+"$SOURCE_DIR"}
    fi
    echo "No libroxel-hub here: fetching the latest release of $REPO."
    exec /usr/local/bin/libroxel-hub-update --now
fi
chmod +x "$binary"
id libroxel >/dev/null 2>&1 || useradd --system --home-dir /var/lib/libroxel --shell /usr/sbin/nologin libroxel
install -d -o libroxel -g libroxel -m 750 /var/lib/libroxel /var/lib/libroxel/backups
[ -f /usr/local/bin/libroxel-hub ] && cp -p /usr/local/bin/libroxel-hub /usr/local/bin/libroxel-hub.previous
install -m 755 "$binary" /usr/local/bin/libroxel-hub.new
mv -f /usr/local/bin/libroxel-hub.new /usr/local/bin/libroxel-hub
if [ -f "$here/update-hub.sh" ]; then
    install -m 755 "$here/update-hub.sh" /usr/local/bin/libroxel-hub-update
fi

echo "== Game servers"
game_flags=""
if [ -f "$here/libroxel" ]; then
    install -m 755 "$here/libroxel" /usr/local/bin/libroxel.new
    mv -f /usr/local/bin/libroxel.new /usr/local/bin/libroxel
fi
if [ -x /usr/local/bin/libroxel ]; then
    # What the app needs to run, even with no screen (sound and input libraries).
    if ! ldconfig -p | grep -q 'libasound.so.2' || ! ldconfig -p | grep -q 'libudev.so.1'; then
        apt-get update -q
        DEBIAN_FRONTEND=noninteractive apt-get install -y -q libudev1 \
            || true
        DEBIAN_FRONTEND=noninteractive apt-get install -y -q libasound2t64 2>/dev/null \
            || DEBIAN_FRONTEND=noninteractive apt-get install -y -q libasound2
    fi
    game_flags="--starters /usr/local/bin/libroxel"
fi
if [ "$GAME_SERVERS" = yes ]; then
    # The game server is the Libroxel app itself, run as `libroxel server` with no window.
    if [ -x /usr/local/bin/libroxel ]; then
        install -d -o libroxel -g libroxel -m 750 /var/lib/libroxel/game-servers
        game_flags="$game_flags --game-servers /usr/local/bin/libroxel --game-server-dir /var/lib/libroxel/game-servers"
        game_flags="$game_flags --max-game-servers $MAX_GAME_SERVERS --players-per-server $PLAYERS_PER_SERVER"
        game_flags="$game_flags --first-udp-port $FIRST_UDP_PORT"
        echo "Up to $MAX_GAME_SERVERS game servers of $PLAYERS_PER_SERVER players each."
    else
        echo "No libroxel app here yet, so no game servers: Play falls back to players hosting."
        echo "(libroxel-hub-update installs it: from source it builds it, from releases it unpacks it.)"
    fi
else
    echo "Off (GAME_SERVERS=no in $conf): players host every game themselves."
fi

echo "== Mail settings in /etc/libroxel/mail.conf (for email codes)"
if [ ! -f /etc/libroxel/mail.conf ]; then
    install -m 600 /dev/null /etc/libroxel/mail.conf
    cat > /etc/libroxel/mail.conf <<'MAIL'
# How the Hub mails codes that prove players' emails. Fill these in from your
# mail provider (Resend, Brevo, Postmark…), then: sudo systemctl restart libroxel-hub
# Until LIBROXEL_SMTP_HOST is set, emails are kept but no codes are sent.
#LIBROXEL_SMTP_HOST=smtp.resend.com
#LIBROXEL_SMTP_PORT=587
#LIBROXEL_SMTP_USER=resend
#LIBROXEL_SMTP_PASSWORD=your-api-key
#LIBROXEL_MAIL_FROM=Libroxel <no-reply@libroxel.com>
#LIBROXEL_MODERATION_KEY=your-openai-key
MAIL
fi

echo "== The service: starts on boot, restarts if it ever stops"
cat > /etc/systemd/system/libroxel-hub.service <<UNIT
[Unit]
Description=Libroxel Hub
After=network-online.target
Wants=network-online.target
StartLimitIntervalSec=0

[Service]
User=libroxel
Group=libroxel
EnvironmentFile=-/etc/libroxel/mail.conf
ExecStart=/usr/local/bin/libroxel-hub --port 7430 --data /var/lib/libroxel/hub.sqlite --behind-proxy $game_flags
Environment=HOME=/var/lib/libroxel
Restart=always
RestartSec=2
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=/var/lib/libroxel
# Game servers are the Hub's children. Restarting the Hub (as every update
# does) leaves them running, so nobody is thrown out of a game; each stops
# on its own once empty. If memory runs short, a game server goes first.
KillMode=process
OOMPolicy=continue

[Install]
WantedBy=multi-user.target
UNIT

echo "== Nightly backups (the last 14 are kept in /var/lib/libroxel/backups)"
cat > /etc/systemd/system/libroxel-hub-backup.service <<UNIT
[Unit]
Description=Back up the Libroxel Hub

[Service]
Type=oneshot
User=libroxel
ExecStart=/bin/sh -c 'sqlite3 /var/lib/libroxel/hub.sqlite ".backup /var/lib/libroxel/backups/hub-\$(date +%%F).sqlite" && find /var/lib/libroxel/backups -name "hub-*.sqlite" -mtime +14 -delete'
UNIT
cat > /etc/systemd/system/libroxel-hub-backup.timer <<UNIT
[Unit]
Description=Back up the Libroxel Hub every night

[Timer]
OnCalendar=*-*-* 04:00:00
Persistent=true

[Install]
WantedBy=timers.target
UNIT

echo "== Updates: every 10 minutes, look for a new version on GitHub"
cat > /etc/systemd/system/libroxel-hub-update.service <<UNIT
[Unit]
Description=Update the Libroxel Hub from GitHub
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/libroxel-hub-update
UNIT
cat > /etc/systemd/system/libroxel-hub-update.timer <<UNIT
[Unit]
Description=Look for Libroxel Hub updates

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
cat > /etc/caddy/sites/hub.caddy <<CADDY
$DOMAIN {
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

# Shortcut for looking after it: sudo libroxel-admin accounts, owners, ban…
cat > /usr/local/bin/libroxel-admin <<'ADMIN'
#!/bin/sh
exec sudo -u libroxel /usr/local/bin/libroxel-hub "$@" --data /var/lib/libroxel/hub.sqlite
ADMIN
chmod 755 /usr/local/bin/libroxel-admin

systemctl daemon-reload
systemctl enable --now libroxel-hub libroxel-hub-backup.timer >/dev/null
if [ -n "$REPO" ] && [ -x /usr/local/bin/libroxel-hub-update ]; then
    systemctl enable --now libroxel-hub-update.timer >/dev/null
fi
systemctl restart libroxel-hub
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
