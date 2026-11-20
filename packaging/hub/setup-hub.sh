#!/usr/bin/env bash
# Sets up (or updates) the official Libroxel Hub on an Ubuntu server.
#
#   sudo bash setup-hub.sh [domain]
#
# Put the libroxel-hub program next to this script first. Run it again
# with a new libroxel-hub to update; accounts and everything else stay.
# The domain (hub.libroxel.com unless given) must already point at this
# server, so Caddy can get its HTTPS certificate.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
[ "$(id -u)" = 0 ] || { echo "Run it with sudo: sudo bash $0 $*"; exit 1; }
[ -x "$here/libroxel-hub" ] || chmod +x "$here/libroxel-hub" 2>/dev/null || { echo "Put libroxel-hub next to this script."; exit 1; }

domain="${1:-hub.libroxel.com}"
echo "Setting up the Libroxel Hub at https://$domain"

echo "== Packages (Caddy for HTTPS, sqlite3 for backups, ufw firewall)"
apt-get update -q
DEBIAN_FRONTEND=noninteractive apt-get install -y -q caddy sqlite3 ufw curl

echo "== The Hub program and its data folder"
install -m 755 "$here/libroxel-hub" /usr/local/bin/libroxel-hub
id libroxel >/dev/null 2>&1 || useradd --system --home-dir /var/lib/libroxel --shell /usr/sbin/nologin libroxel
install -d -o libroxel -g libroxel -m 750 /var/lib/libroxel /var/lib/libroxel/backups

cat > /etc/systemd/system/libroxel-hub.service <<UNIT
[Unit]
Description=Libroxel Hub
After=network-online.target
Wants=network-online.target

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

echo "== HTTPS with Caddy"
# Each site gets its own file in /etc/caddy/sites, so the website can sit beside the Hub.
install -d /etc/caddy/sites
grep -q 'import /etc/caddy/sites/\*' /etc/caddy/Caddyfile 2>/dev/null \
    || echo 'import /etc/caddy/sites/*.caddy' > /etc/caddy/Caddyfile
cat > /etc/caddy/sites/hub.caddy <<CADDY
$domain {
    encode gzip
    reverse_proxy 127.0.0.1:7430
}
CADDY

echo "== Firewall: SSH, HTTP and HTTPS only"
ufw allow OpenSSH >/dev/null
ufw allow 80/tcp >/dev/null
ufw allow 443/tcp >/dev/null
ufw --force enable >/dev/null

systemctl daemon-reload
systemctl enable --now libroxel-hub libroxel-hub-backup.timer
systemctl restart libroxel-hub
systemctl enable caddy >/dev/null 2>&1 || true
systemctl reload caddy || systemctl restart caddy

# Shortcut for looking after it: sudo libroxel-admin accounts, owners, ban…
cat > /usr/local/bin/libroxel-admin <<'ADMIN'
#!/bin/sh
exec sudo -u libroxel /usr/local/bin/libroxel-hub "$@" --data /var/lib/libroxel/hub.sqlite
ADMIN
chmod 755 /usr/local/bin/libroxel-admin

echo "== Checking it answers (the HTTPS certificate can take a minute the first time)"
for _ in $(seq 1 30); do
    if curl -fsS "https://$domain/" >/dev/null 2>&1; then
        curl -fsS "https://$domain/"; echo
        echo "Done: the Hub is live at https://$domain"
        exit 0
    fi
    sleep 4
done
echo "The Hub runs, but https://$domain doesn't answer yet. Check: sudo journalctl -u caddy -n 50"
