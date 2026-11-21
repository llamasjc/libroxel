#!/bin/sh
# The Libroxel installer for Linux. Run it with:
#
#   sh libroxel-<version>-linux-x86_64.run              install (or update)
#   sh libroxel-<version>-linux-x86_64.run --uninstall  take it out again
#
# It installs for you alone (no password): the app in
# ~/.local/share/libroxel, an entry in the app menu, and the `libroxel`
# command in ~/.local/bin. The app itself is packed after this script.
set -eu
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
line=$(awk '/^__LIBROXEL_PAYLOAD__$/ { print NR + 1; exit }' "$0")
tail -n +"$line" "$0" | tar -xz -C "$tmp"
sh "$tmp"/libroxel/install.sh "$@"
exit 0
__LIBROXEL_PAYLOAD__
