#!/bin/sh
# Installs Libroxel for this user, so it shows up in the app menu and
# `libroxel` in a terminal opens it. No password needed.
#
#   ./install.sh              install (or update) from this folder
#   ./install.sh --uninstall  take it out again
#
# Just running ./libroxel works too: the app makes the `libroxel` command
# itself the first time it opens.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
app_dir="$data/libroxel/app"
bin="$HOME/.local/bin"
desktop="$data/applications/libroxel.desktop"

if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$bin/libroxel" "$desktop"
  rm -rf "$app_dir"
  echo "Libroxel is uninstalled. (Your games and settings are kept.)"
  exit 0
fi

if [ ! -x "$here/libroxel" ]; then
  echo "install.sh: run it from the unpacked Libroxel folder (no libroxel here)" >&2
  exit 1
fi

mkdir -p "$app_dir" "$bin" "$(dirname "$desktop")"
cp "$here/libroxel" "$app_dir/libroxel.new"
mv -f "$app_dir/libroxel.new" "$app_dir/libroxel"
for f in README.md LICENSE-MIT LICENSE-APACHE; do
  [ -f "$here/$f" ] && cp "$here/$f" "$app_dir/"
done
ln -sfn "$app_dir/libroxel" "$bin/libroxel"

cat > "$desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Libroxel
Comment=Make and play 3D games with friends
Exec="$app_dir/libroxel" %u
Terminal=false
Categories=Game;Development;
MimeType=x-scheme-handler/libroxel;
DESKTOP
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" >/dev/null 2>&1 || true

# Put ~/.local/bin on PATH for new terminals, if it isn't already.
case ":$PATH:" in
  *":$bin:"*) on_path=yes ;;
  *) on_path=no ;;
esac
if [ "$on_path" = no ]; then
  added=no
  for rc in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
    if [ -f "$rc" ] && ! grep -q "Added by Libroxel" "$rc"; then
      printf '\n# Added by Libroxel, so the `libroxel` command works.\ncase ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac\n' >> "$rc"
      added=yes
    elif [ -f "$rc" ]; then
      added=yes
    fi
  done
  if [ "$added" = no ]; then
    printf '\n# Added by Libroxel, so the `libroxel` command works.\ncase ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac\n' >> "$HOME/.profile"
  fi
  echo "Libroxel is installed. Open a new terminal and type: libroxel"
else
  echo "Libroxel is installed. Type: libroxel"
fi
