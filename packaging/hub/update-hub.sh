#!/usr/bin/env bash
# Updates the Libroxel Hub from the newest GitHub release (full or Hub-only) of the repository
# in /etc/libroxel/hub.conf. A timer runs it every 10 minutes; run it by hand
# with: sudo libroxel-hub-update --now
#
# It checks each file against the release's SHA256SUMS.txt, backs the
# database up, sets up with the release's own setup-hub.sh, and puts the old
# Hub back if the new one doesn't answer within 30 seconds (and then leaves
# that release alone).
set -euo pipefail

[ "$(id -u)" = 0 ] || { echo "Run it with sudo."; exit 1; }
conf=/etc/libroxel/hub.conf
[ -f "$conf" ] || { echo "No $conf: run setup-hub.sh first."; exit 1; }
REPO=""
GITHUB_TOKEN=""
. "$conf"
[ -n "$REPO" ] || { echo "No REPO in $conf, so there is nothing to update from."; exit 0; }

state=/var/lib/libroxel-update
install -d -m 755 "$state"
log() { echo "libroxel-hub-update: $*"; }

auth=()
[ -n "$GITHUB_TOKEN" ] && auth=(-H "Authorization: Bearer $GITHUB_TOKEN")
# GitHub answers with JSON, or with the file itself for application/octet-stream.
api() { local accept="$1"; shift; curl -fsSL --retry 3 -H "Accept: $accept" "${auth[@]}" "$@"; }

# The newest release that carries the Hub: a full release (v…) or a Hub-only one (hub-v…).
releases="$(api application/vnd.github+json "https://api.github.com/repos/$REPO/releases?per_page=30")" \
    || { log "could not ask GitHub about $REPO's releases"; exit 1; }
release="$(jq -c '[.[] | select((.draft | not) and (.prerelease | not))
    | select(any(.assets[]; .name == "libroxel-hub-linux-x86_64"))]
    | sort_by(.published_at) | last // empty' <<<"$releases")"
[ -n "$release" ] || { log "no release of $REPO carries the Hub yet"; exit 0; }
tag="$(jq -r .tag_name <<<"$release")"
current="$(cat "$state/release" 2>/dev/null || true)"
if [ "$tag" = "$current" ]; then
    [ "${1:-}" = --now ] && log "already on $tag"
    exit 0
fi
if [ "$tag" = "$(cat "$state/skipped" 2>/dev/null || true)" ]; then
    exit 0
fi
asset_url() { jq -r --arg n "$1" '.assets[] | select(.name == $n) | .url' <<<"$release"; }
for name in libroxel-hub-linux-x86_64 setup-hub.sh update-hub.sh SHA256SUMS.txt; do
    [ -n "$(asset_url "$name")" ] || { log "release $tag has no $name; leaving the Hub as it is"; exit 0; }
done

log "updating to $tag"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
for name in libroxel-hub-linux-x86_64 setup-hub.sh update-hub.sh SHA256SUMS.txt; do
    api application/octet-stream "$(asset_url "$name")" -o "$work/$name"
done
(cd "$work" && grep -E ' (libroxel-hub-linux-x86_64|setup-hub.sh|update-hub.sh)$' SHA256SUMS.txt | sha256sum --check --strict --quiet) \
    || { log "checksums don't match; not installing $tag"; exit 1; }

# A copy of everything first: a new Hub may change the database when it starts.
if [ -f /var/lib/libroxel/hub.sqlite ]; then
    sudo -u libroxel sqlite3 /var/lib/libroxel/hub.sqlite ".backup /var/lib/libroxel/backups/before-$tag.sqlite"
fi
[ -f /usr/local/bin/libroxel-hub ] && cp -p /usr/local/bin/libroxel-hub "$state/libroxel-hub.previous"

bash "$work/setup-hub.sh" --no-check

healthy=no
for _ in $(seq 1 15); do
    if curl -fsS http://127.0.0.1:7430/ >/dev/null 2>&1; then
        healthy=yes
        break
    fi
    sleep 2
done
if [ "$healthy" = yes ]; then
    echo "$tag" > "$state/release"
    log "the Hub is on $tag"
else
    log "the Hub didn't start on $tag; putting the old one back"
    if [ -f "$state/libroxel-hub.previous" ]; then
        install -m 755 "$state/libroxel-hub.previous" /usr/local/bin/libroxel-hub
    fi
    systemctl restart libroxel-hub
    echo "$tag" > "$state/skipped"
    exit 1
fi
