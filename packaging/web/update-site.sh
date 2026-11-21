#!/usr/bin/env bash
# Puts the website from the latest GitHub release online, so it matches the
# released app: fetches that release's tag of the repository in
# /etc/libroxel/site.conf and, when it changed, sets the site up again with
# the fetched copy's own setup-site.sh. A timer runs it every
# 10 minutes; run it by hand with: sudo libroxel-site-update --now
set -euo pipefail

[ "$(id -u)" = 0 ] || { echo "Run it with sudo."; exit 1; }
conf=/etc/libroxel/site.conf
[ -f "$conf" ] || { echo "No $conf: run setup-site.sh first."; exit 1; }
REPO=""
. "$conf"
[ -n "$REPO" ] || { echo "No REPO in $conf, so there is nothing to update from."; exit 0; }

log() { echo "libroxel-site-update: $*"; }
checkout=/var/lib/libroxel-site/repo
if [ ! -d "$checkout/.git" ]; then
    rm -rf "$checkout"
    install -d -m 755 /var/lib/libroxel-site
    git clone -q --depth 1 --filter=blob:none --sparse "https://github.com/$REPO.git" "$checkout"
    git -C "$checkout" sparse-checkout set web packaging/web
fi
git -C "$checkout" remote set-url origin "https://github.com/$REPO.git"
done_file=/var/lib/libroxel-site/online
before="$(cat "$done_file" 2>/dev/null || true)"
tag="$(curl -fsSL --retry 3 -H "Accept: application/vnd.github+json" \
    "https://api.github.com/repos/$REPO/releases/latest" \
    | grep -o '"tag_name": *"[^"]*"' | head -n 1 | sed 's/.*"\([^"]*\)"$/\1/')" || true
[ -n "$tag" ] || { log "no release of $REPO yet; leaving the site as it is"; exit 0; }
now="$tag"
if [ "$now" = "$before" ] && [ "${1:-}" != --now ]; then
    exit 0
fi
git -C "$checkout" fetch -q --depth 1 origin "refs/tags/$tag" || { log "could not fetch $tag from github.com/$REPO"; exit 1; }
git -C "$checkout" reset -q --hard FETCH_HEAD
[ -f "$checkout/web/index.html" ] || { log "$tag has no web/index.html; leaving the site as it is"; exit 1; }
log "putting $now online"
bash "$checkout/packaging/web/setup-site.sh" --from "$checkout/web" --no-check
echo "$now" > "$done_file"
log "the website is on $now"
