#!/usr/bin/env bash
# Updates the PlayLibre Hub from GitHub. A timer runs it every 10 minutes.
#
#   sudo playlibre-hub-update --from-source   build main on this server and install it now,
#                                            and from then on whenever main changes
#   sudo playlibre-hub-update --from-releases go back to installing GitHub releases
#   sudo playlibre-hub-update --now           update now, whichever way is set
#
# From source, it pulls the repository in /etc/playlibre/hub.conf into a
# folder of the user who ran sudo (~/playlibre), builds the Hub there with
# cargo (installing Rust for that user the first time), and installs it: no
# release needed. From releases, it takes the newest GitHub release (full or
# Hub-only) and checks each file against its SHA256SUMS.txt.
#
# Either way it backs the database up first, sets up with that version's own
# setup-hub.sh, and puts the old Hub back if the new one doesn't answer
# within 30 seconds (and then tries that version again an hour later).
set -euo pipefail

[ "$(id -u)" = 0 ] || { echo "Run it with sudo."; exit 1; }
conf=/etc/playlibre/hub.conf
[ -f "$conf" ] || { echo "No $conf: run setup-hub.sh first."; exit 1; }
REPO=""
GITHUB_TOKEN=""
UPDATE_FROM=releases
SOURCE_USER=""
SOURCE_DIR=""
GAME_SERVERS=yes
. "$conf"
[ -n "$REPO" ] || { echo "No REPO in $conf, so there is nothing to update from."; exit 0; }

state=/var/lib/playlibre-update
install -d -m 755 "$state"
log() { echo "playlibre-hub-update: $*"; }

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
        SOURCE_DIR="${2:-$home/playlibre}"
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
    --now) by_hand=yes; rm -f "$state/skipped" ;;
esac
skipped() { [ "$1" = "$(cat "$state/skipped" 2>/dev/null || true)" ] && [ -n "$(find "$state/skipped" -mmin -60)" ]; }

# Puts the Hub in $1 (with setup-hub.sh and update-hub.sh beside it) in
# place as version $2, and keeps it only if it answers.
install_hub() {
    local dir="$1" version="$2"
    log "installing $version"
    # A copy of everything first: a new Hub may change the database when it starts.
    if [ -f /var/lib/playlibre/hub.sqlite ]; then
        sudo -u playlibre sqlite3 /var/lib/playlibre/hub.sqlite \
            ".backup /var/lib/playlibre/backups/before-${version//\//-}.sqlite"
    fi
    [ -f /usr/local/bin/playlibre-hub ] && cp -p /usr/local/bin/playlibre-hub "$state/playlibre-hub.previous"

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
        if [ -n "${commit:-}" ]; then echo "$commit" > "$state/commit"; else rm -f "$state/commit"; fi
        log "the Hub is on $version"
    else
        log "the Hub didn't start on $version; putting the old one back"
        if [ -f "$state/playlibre-hub.previous" ]; then
            install -m 755 "$state/playlibre-hub.previous" /usr/local/bin/playlibre-hub
        fi
        systemctl restart playlibre-hub
        echo "$version" > "$state/skipped"
        exit 1
    fi
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ "$UPDATE_FROM" = source ]; then
    user="${SOURCE_USER:-root}"
    dir="${SOURCE_DIR:-/root/playlibre}"
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
    # Follows REPO when it changes (such as after the repository's rename).
    as_user "cd '$dir' && git remote set-url origin '$url' && git fetch --quiet origin main"
    commit="$(as_user "cd '$dir' && git rev-parse origin/main")"
    version="main-${commit:0:7}"
    current="$(cat "$state/release" 2>/dev/null || true)"
    if [ "$by_hand" = no ] && { [ "$version" = "$current" ] || skipped "$version"; }; then
        exit 0
    fi
    log "building $version in $dir (the first time takes a few minutes)"
    as_user "cd '$dir' && git checkout --quiet main && git reset --quiet --hard origin/main \
        && PATH=\"\$HOME/.cargo/bin:\$PATH\" cargo build --release --locked -p playlibre-hub --bin playlibre-hub"
    cp "$dir/target/release/playlibre-hub" "$dir/packaging/hub/setup-hub.sh" "$dir/packaging/hub/update-hub.sh" "$work/"
    if [ "$GAME_SERVERS" = yes ]; then
        # The game server is the whole app, which needs a few libraries to build.
        if ! dpkg -s libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev pkg-config >/dev/null 2>&1; then
            log "installing what the game server needs to build"
            apt-get update -q && DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
                pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
        fi
        # Building it takes a few GB of memory: add swap on a small server that has none.
        memory_kb="$(awk '/MemTotal/ {print $2}' /proc/meminfo)"
        if [ "$memory_kb" -lt 6000000 ] && [ -z "$(swapon --noheadings)" ] && [ ! -f /swapfile ]; then
            log "adding 4 GB of swap for building"
            fallocate -l 4G /swapfile && chmod 600 /swapfile && mkswap /swapfile >/dev/null && swapon /swapfile \
                && echo '/swapfile none swap sw 0 0' >> /etc/fstab
        fi
        jobs=$(( memory_kb / 2000000 )); [ "$jobs" -ge 1 ] || jobs=1
        log "building the game server (the first time takes a while)"
        if as_user "cd '$dir' && PATH=\"\$HOME/.cargo/bin:\$PATH\" nice cargo build --release --locked -j $jobs -p playlibre --bin playlibre"; then
            cp "$dir/target/release/playlibre" "$work/"
        else
            log "the game server didn't build; keeping the one there is (if any)"
        fi
    fi
    install_hub "$work" "$version"
    exit 0
fi

auth=()
[ -n "$GITHUB_TOKEN" ] && auth=(-H "Authorization: Bearer $GITHUB_TOKEN")
# GitHub answers with JSON, or with the file itself for application/octet-stream.
api() { local accept="$1"; shift; curl -fsSL --retry 3 -H "Accept: $accept" "${auth[@]}" "$@"; }

# The newest release that carries the Hub.
releases="$(api application/vnd.github+json "https://api.github.com/repos/$REPO/releases?per_page=30")" \
    || { log "could not ask GitHub about $REPO's releases"; exit 1; }
release="$(jq -c '[.[] | select((.draft | not) and (.prerelease | not))
    | select(any(.assets[]; .name == "playlibre-hub-linux-x86_64" or .name == "libroxel-hub-linux-x86_64"))]
    | sort_by(.published_at) | last // empty' <<<"$releases")"
[ -n "$release" ] || { log "no release of $REPO carries the Hub yet"; exit 0; }
tag="$(jq -r .tag_name <<<"$release")"
current="$(cat "$state/release" 2>/dev/null || true)"
if [ "$tag" = "$current" ]; then
    [ "$by_hand" = yes ] && log "already on $tag"
    exit 0
fi
if skipped "$tag"; then
    exit 0
fi
asset_url() { jq -r --arg n "$1" '.assets[] | select(.name == $n) | .url' <<<"$release"; }
# Releases from before the rename (Libroxel) call the Hub libroxel-hub-linux-x86_64.
hub_file=playlibre-hub-linux-x86_64
[ -n "$(asset_url "$hub_file")" ] || hub_file=libroxel-hub-linux-x86_64
for name in "$hub_file" setup-hub.sh update-hub.sh SHA256SUMS.txt; do
    [ -n "$(asset_url "$name")" ] || { log "release $tag has no $name; leaving the Hub as it is"; exit 0; }
done

log "updating to $tag"
for name in "$hub_file" setup-hub.sh update-hub.sh SHA256SUMS.txt; do
    api application/octet-stream "$(asset_url "$name")" -o "$work/$name"
done
(cd "$work" && grep -E " ($hub_file|setup-hub.sh|update-hub.sh)\$" SHA256SUMS.txt | sha256sum --check --strict --quiet) \
    || { log "checksums don't match; not installing $tag"; exit 1; }
# The app, from the release's Debian package.
app="$(jq -r '.assets[] | select(.name | test("^(playlibre|libroxel)-.*linux-x86_64\\.deb$")) | .name' <<<"$release" | head -n 1)"
if [ -n "$app" ]; then
    api application/octet-stream "$(asset_url "$app")" -o "$work/$app"
    if (cd "$work" && grep -F " $app" SHA256SUMS.txt | sha256sum --check --strict --quiet); then
        dpkg-deb -x "$work/$app" "$work/app"
        found="$(find "$work/app" -type f \( -name playlibre -o -name libroxel \) -perm -u+x | head -n 1)"
        [ -n "$found" ] && cp "$found" "$work/playlibre"
    else
        log "the app's checksum doesn't match; leaving game servers as they are"
    fi
fi
install_hub "$work" "$tag"
