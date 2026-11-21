#!/usr/bin/env bash
# Updates the Libroxel Hub from GitHub. A timer runs it every 10 minutes.
#
#   sudo libroxel-hub-update --from-source   build main on this server and install it now,
#                                            and from then on whenever main changes
#   sudo libroxel-hub-update --from-releases go back to installing GitHub releases
#   sudo libroxel-hub-update --now           update now, whichever way is set
#
# From source, it pulls the repository in /etc/libroxel/hub.conf into a
# folder of the user who ran sudo (~/libroxel), builds the Hub there with
# cargo (installing Rust for that user the first time), and installs it: no
# release needed. From releases, it takes the newest GitHub release (full or
# Hub-only) and checks each file against its SHA256SUMS.txt.
#
# Either way it backs the database up first, sets up with that version's own
# setup-hub.sh, and puts the old Hub back if the new one doesn't answer
# within 30 seconds (and then leaves that version alone).
set -euo pipefail

[ "$(id -u)" = 0 ] || { echo "Run it with sudo."; exit 1; }
conf=/etc/libroxel/hub.conf
[ -f "$conf" ] || { echo "No $conf: run setup-hub.sh first."; exit 1; }
REPO=""
GITHUB_TOKEN=""
UPDATE_FROM=releases
SOURCE_USER=""
SOURCE_DIR=""
. "$conf"
[ -n "$REPO" ] || { echo "No REPO in $conf, so there is nothing to update from."; exit 0; }

state=/var/lib/libroxel-update
install -d -m 755 "$state"
log() { echo "libroxel-hub-update: $*"; }

by_hand=no
set_conf() { # key value: change one setting in hub.conf
    if grep -q "^$1=" "$conf"; then
        sed -i "s|^$1=.*|$1=\"$2\"|" "$conf"
    else
        echo "$1=\"$2\"" >> "$conf"
    fi
}
case "${1:-}" in
    --from-source)
        by_hand=yes
        UPDATE_FROM=source
        SOURCE_USER="${SUDO_USER:-root}"
        [ "$SOURCE_USER" = root ] && home=/root || home="$(getent passwd "$SOURCE_USER" | cut -d: -f6)"
        SOURCE_DIR="${2:-$home/libroxel}"
        set_conf UPDATE_FROM source
        set_conf SOURCE_USER "$SOURCE_USER"
        set_conf SOURCE_DIR "$SOURCE_DIR"
        rm -f "$state/skipped"
        ;;
    --from-releases)
        by_hand=yes
        UPDATE_FROM=releases
        set_conf UPDATE_FROM releases
        rm -f "$state/skipped"
        ;;
    --now) by_hand=yes ;;
esac

# Puts the Hub in $1 (with setup-hub.sh and update-hub.sh beside it) in
# place as version $2, and keeps it only if it answers.
install_hub() {
    local dir="$1" version="$2"
    log "installing $version"
    # A copy of everything first: a new Hub may change the database when it starts.
    if [ -f /var/lib/libroxel/hub.sqlite ]; then
        sudo -u libroxel sqlite3 /var/lib/libroxel/hub.sqlite \
            ".backup /var/lib/libroxel/backups/before-${version//\//-}.sqlite"
    fi
    [ -f /usr/local/bin/libroxel-hub ] && cp -p /usr/local/bin/libroxel-hub "$state/libroxel-hub.previous"

    bash "$dir/setup-hub.sh" --no-check

    local healthy=no
    for _ in $(seq 1 15); do
        if curl -fsS http://127.0.0.1:7430/ >/dev/null 2>&1; then
            healthy=yes
            break
        fi
        sleep 2
    done
    if [ "$healthy" = yes ]; then
        echo "$version" > "$state/release"
        log "the Hub is on $version"
    else
        log "the Hub didn't start on $version; putting the old one back"
        if [ -f "$state/libroxel-hub.previous" ]; then
            install -m 755 "$state/libroxel-hub.previous" /usr/local/bin/libroxel-hub
        fi
        systemctl restart libroxel-hub
        echo "$version" > "$state/skipped"
        exit 1
    fi
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ "$UPDATE_FROM" = source ]; then
    user="${SOURCE_USER:-root}"
    dir="${SOURCE_DIR:-/root/libroxel}"
    as_user() { sudo -u "$user" -H bash -lc "$1"; }
    for tool in git cc; do
        command -v "$tool" >/dev/null || {
            log "installing git and a C compiler"
            apt-get update -q && DEBIAN_FRONTEND=noninteractive apt-get install -y -q git build-essential
            break
        }
    done
    if ! as_user 'command -v cargo >/dev/null || [ -x ~/.cargo/bin/cargo ]'; then
        log "installing Rust for $user"
        as_user 'curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal'
    fi
    url="https://github.com/$REPO.git"
    [ -n "$GITHUB_TOKEN" ] && url="https://x-access-token:$GITHUB_TOKEN@github.com/$REPO.git"
    if [ ! -d "$dir/.git" ]; then
        log "fetching $REPO into $dir"
        as_user "git clone --quiet '$url' '$dir'"
    fi
    as_user "cd '$dir' && git fetch --quiet origin main"
    commit="$(as_user "cd '$dir' && git rev-parse origin/main")"
    version="main-${commit:0:7}"
    current="$(cat "$state/release" 2>/dev/null || true)"
    if [ "$by_hand" = no ] && { [ "$version" = "$current" ] || [ "$version" = "$(cat "$state/skipped" 2>/dev/null || true)" ]; }; then
        exit 0
    fi
    log "building $version in $dir (the first time takes a few minutes)"
    as_user "cd '$dir' && git checkout --quiet main && git reset --quiet --hard origin/main \
        && PATH=\"\$HOME/.cargo/bin:\$PATH\" cargo build --release --locked -p libroxel-hub --bin libroxel-hub"
    cp "$dir/target/release/libroxel-hub" "$dir/packaging/hub/setup-hub.sh" "$dir/packaging/hub/update-hub.sh" "$work/"
    install_hub "$work" "$version"
    exit 0
fi

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
    [ "$by_hand" = yes ] && log "already on $tag"
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
for name in libroxel-hub-linux-x86_64 setup-hub.sh update-hub.sh SHA256SUMS.txt; do
    api application/octet-stream "$(asset_url "$name")" -o "$work/$name"
done
(cd "$work" && grep -E ' (libroxel-hub-linux-x86_64|setup-hub.sh|update-hub.sh)$' SHA256SUMS.txt | sha256sum --check --strict --quiet) \
    || { log "checksums don't match; not installing $tag"; exit 1; }
install_hub "$work" "$tag"
