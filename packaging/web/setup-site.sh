#!/usr/bin/env bash
# Puts the Libroxel website (the web/ folder) on an Ubuntu server, served by
# Caddy with HTTPS, next to the official Hub.
#
#   sudo bash setup-site.sh --repo <owner>/<repo> [--domain playlibre.net] [--hub https://hub.playlibre.net]
#
# With --repo the site comes from that GitHub repository's main branch, and
# a timer checks it every 10 minutes and puts any change online (see
# update-site.sh). Without it, the web/ folder next to this script (or the
# repository's own, two folders up) goes online as it is.
#
# Run it again to change the settings; downloads in /var/www/libroxel/downloads
# stay. The domain and www.<domain> must already point at this server, so
# Caddy can get their certificates.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
[ "$(id -u)" = 0 ] || { echo "Run it with sudo: sudo bash $0 $*"; exit 1; }

conf=/etc/libroxel/site.conf
REPO=""
DOMAIN="playlibre.net"
HUB="https://hub.playlibre.net"
[ -f "$conf" ] && . "$conf"
from=""
check=yes
while [ $# -gt 0 ]; do
    case "$1" in
        --repo) REPO="$2"; shift 2 ;;
        --domain) DOMAIN="$2"; shift 2 ;;
        --hub) HUB="${2%/}"; shift 2 ;;
        # The web folder to put online (the updater passes its checkout).
        --from) from="$2"; shift 2 ;;
        --no-check) check=no; shift ;;
        *) echo "unknown option $1"; exit 1 ;;
    esac
done
case "$HUB" in https://*) ;; *) echo "The Hub address must start with https://"; exit 1 ;; esac
case "$REPO" in ""|*/*) ;; *) echo "--repo is owner/name, such as someone/libroxel"; exit 1 ;; esac

install -d -m 755 /etc/libroxel
cat > "$conf" <<CONF
# The Libroxel website (setup-site.sh writes this; run it again to change it).
# The GitHub repository whose latest release the site updates from (owner/name).
REPO="$REPO"
DOMAIN="$DOMAIN"
HUB="$HUB"
CONF

need=()
command -v caddy >/dev/null || need+=(caddy)
command -v curl >/dev/null || need+=(curl)
[ -n "$REPO" ] && ! command -v git >/dev/null && need+=(git)
if [ ${#need[@]} -gt 0 ]; then
    apt-get update -q
    DEBIAN_FRONTEND=noninteractive apt-get install -y -q "${need[@]}"
fi

# From GitHub: fetch main, then the fetched copy's own scripts put it online.
if [ -n "$REPO" ] && [ -z "$from" ]; then
    checkout=/var/lib/libroxel-site/repo
    if [ ! -d "$checkout/.git" ]; then
        echo "== Getting the website from github.com/$REPO"
        rm -rf "$checkout"
        install -d -m 755 /var/lib/libroxel-site
        git clone -q --depth 1 --filter=blob:none --sparse "https://github.com/$REPO.git" "$checkout"
        git -C "$checkout" sparse-checkout set web packaging/web
    fi
    [ -f "$checkout/packaging/web/update-site.sh" ] \
        || { echo "github.com/$REPO's main has no packaging/web/update-site.sh yet: push the website first."; exit 1; }
    bash "$checkout/packaging/web/update-site.sh" --now
fi

if [ -z "$REPO" ] || [ -n "$from" ]; then
site="$from"
if [ -z "$site" ]; then
    for candidate in "$here/web" "$here/../../web"; do
        [ -f "$candidate/index.html" ] && site="$candidate" && break
    done
fi
[ -n "$site" ] && [ -f "$site/index.html" ] || { echo "Put the web folder next to this script, or use --repo."; exit 1; }
site="$(cd "$site" && pwd)"

echo "== The site's files, in /var/www/libroxel (Hub: $HUB)"
install -d -m 755 /var/www/libroxel /var/www/libroxel/downloads
new="$(mktemp -d /var/www/.libroxel-new.XXXXXX)"
cp -r "$site"/. "$new"/
# Point the site at this Hub, and at the repository's releases for downloads.
sed -i -e "s#<meta name=\"libroxel-hub\" content=\"[^\"]*\">#<meta name=\"libroxel-hub\" content=\"$HUB\">#" \
    -e "s#<meta name=\"libroxel-repo\" content=\"[^\"]*\">#<meta name=\"libroxel-repo\" content=\"$REPO\">#" \
    "$new/index.html"
sed -i "s#https://playlibre.net#https://$DOMAIN#g" "$new/index.html" "$new/robots.txt" "$new/sitemap.xml"
chmod -R a+rX "$new"
# Swap the files in, keeping the downloads; files gone from the site go.
find /var/www/libroxel -mindepth 1 -maxdepth 1 ! -name downloads -exec rm -rf {} +
find "$new" -mindepth 1 -maxdepth 1 ! -name downloads -exec mv -t /var/www/libroxel {} +
rm -rf "$new"

echo "== HTTPS with Caddy (/etc/caddy/sites/site.caddy)"
install -d /etc/caddy/sites
# The Hub's setup makes the Caddyfile read every file in sites/. If the
# website goes on first, do the same, keeping a copy of what was there.
if ! grep -q 'import /etc/caddy/sites/\*' /etc/caddy/Caddyfile 2>/dev/null; then
    if [ -s /etc/caddy/Caddyfile ]; then
        cp /etc/caddy/Caddyfile /etc/caddy/Caddyfile.before-libroxel
        echo "   (the old Caddyfile is kept as /etc/caddy/Caddyfile.before-libroxel)"
    fi
    echo 'import /etc/caddy/sites/*.caddy' > /etc/caddy/Caddyfile
fi
github=""
[ -n "$REPO" ] && github=" https://api.github.com"
cat > /etc/caddy/sites/site.caddy.new <<CADDY
$DOMAIN {
    encode zstd gzip
    root * /var/www/libroxel
    header {
        # Only this site's own files run, and they only talk to the Hub (and GitHub, for downloads).
        Content-Security-Policy "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' $HUB$github; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'"
        X-Content-Type-Options nosniff
        Referrer-Policy strict-origin-when-cross-origin
        Strict-Transport-Security "max-age=31536000"
        Permissions-Policy "camera=(), microphone=(), geolocation=()"
    }
    # Pages always ask for the newest files, so an update shows at once.
    header Cache-Control "no-cache"
    @downloads path /downloads/*
    header @downloads Content-Disposition attachment
    try_files {path} /index.html
    file_server
}

www.$DOMAIN {
    redir https://$DOMAIN{uri} permanent
}
CADDY
if ! cmp -s /etc/caddy/sites/site.caddy.new /etc/caddy/sites/site.caddy; then
    mv /etc/caddy/sites/site.caddy.new /etc/caddy/sites/site.caddy
    caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null
    systemctl enable caddy >/dev/null 2>&1 || true
    systemctl reload caddy 2>/dev/null || systemctl restart caddy
else
    rm /etc/caddy/sites/site.caddy.new
fi

if [ -n "$REPO" ]; then
    echo "== Updates from github.com/$REPO every 10 minutes"
    install -m 755 "$site/../packaging/web/update-site.sh" /usr/local/bin/libroxel-site-update
    cat > /etc/systemd/system/libroxel-site-update.service <<UNIT
[Unit]
Description=Update the Libroxel website from GitHub
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/libroxel-site-update
UNIT
    cat > /etc/systemd/system/libroxel-site-update.timer <<UNIT
[Unit]
Description=Update the Libroxel website from GitHub every 10 minutes

[Timer]
OnBootSec=2min
OnUnitActiveSec=10min

[Install]
WantedBy=timers.target
UNIT
    systemctl daemon-reload
    systemctl enable --now libroxel-site-update.timer >/dev/null
fi

fi

if command -v ufw >/dev/null && ufw status | grep -q 'Status: active'; then
    ufw allow 80/tcp >/dev/null
    ufw allow 443/tcp >/dev/null
fi

[ "$check" = yes ] || exit 0
echo "== Checking it answers (the HTTPS certificate can take a minute the first time)"
for _ in $(seq 1 30); do
    if curl -fs -o /dev/null "https://$DOMAIN/"; then
        echo "Done: the website is live at https://$DOMAIN"
        exit 0
    fi
    sleep 4
done
echo "The site is set up, but https://$DOMAIN doesn't answer yet. Check that $DOMAIN and www.$DOMAIN"
echo "point at this server, then: sudo journalctl -u caddy -n 50"
