#!/usr/bin/env bash
# Sets up (or updates) the official Libroxel Hub on an Ubuntu server:
# the Hub as a service that always runs, HTTPS with Caddy, a firewall,
# nightly backups, and updates from GitHub releases.
#
#   sudo bash setup-hub.sh --repo <owner>/<repo> [--domain hub.libroxel.com]
#
# With a libroxel-hub (or libroxel-hub-linux-x86_64) next to this script it
# installs that one; otherwise it downloads the latest release. Run it again
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
[ -f "$conf" ] && . "$conf"
check=yes
while [ $# -gt 0 ]; do
    case "$1" in
        --domain) DOMAIN="$2"; shift 2 ;;
        --repo) REPO="$2"; shift 2 ;;
        --token) GITHUB_TOKEN="$2"; shift 2 ;;
        --no-check) check=no; shift ;;
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
# Only for a private repository: a token that can read its releases.
GITHUB_TOKEN="$GITHUB_TOKEN"
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
ExecStart=/usr/local/bin/libroxel-hub --port 7430 --data /var/lib/libroxel/hub.sqlite --behind-proxy
Restart=always
RestartSec=2
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=/var/lib/libroxel

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

echo "== Updates: every 10 minutes, look for a new GitHub release"
cat > /etc/systemd/system/libroxel-hub-update.service <<UNIT
[Unit]
Description=Update the Libroxel Hub from its latest GitHub release
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
    reverse_proxy 127.0.0.1:7430
}
CADDY

echo "== Firewall: SSH, HTTP and HTTPS only"
ufw allow OpenSSH >/dev/null
ufw allow 80/tcp >/dev/null
ufw allow 443/tcp >/dev/null
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
        [ -n "$REPO" ] && echo "It updates itself from the releases of $REPO."
        exit 0
    fi
    sleep 4
done
echo "The Hub runs, but https://$DOMAIN doesn't answer yet. Check: sudo journalctl -u caddy -n 50"
