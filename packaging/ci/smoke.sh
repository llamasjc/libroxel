#!/usr/bin/env bash
# Launch check for a built app: smoke.sh <path to libroxel> <server|window>
#
# server: `libroxel help` works, and every starter game hosts with no window
#         and shuts down cleanly.
# window: the same, then the app opens its window and draws frames for a
#         while (Linux: under xvfb with Mesa's software Vulkan).
set -euo pipefail
app="$1"
mode="${2:-server}"
seconds="${SMOKE_SECONDS:-20}"

# Its own settings folder, so nothing from the runner leaks in or out.
HOME="$(mktemp -d)"
export HOME
unset XDG_CONFIG_HOME APPDATA XDG_DATA_HOME
export LIBROXEL_NO_UPDATE=1 LIBROXEL_NO_COMMAND=1 RUST_BACKTRACE=1

fail() { echo "::error::$1"; exit 1; }

echo "== libroxel help"
"$app" help | tee help.txt
grep -q "usage:" help.txt || fail "libroxel help printed no usage"

echo "== starter games, hosted with no window"
python=$(command -v python3 || command -v python)
names=$("$app" starters | "$python" -c 'import json,sys; print("\n".join(g["name"] for g in json.load(sys.stdin)))' | tr -d "\r")
[[ -n "$names" ]] || fail "libroxel starters listed no games"
while IFS= read -r name; do
  echo "-- $name"
  set +e
  timeout 120 "$app" server --game "starter:$name" --no-hub --port 0 --stop-when-empty 5 > server.log 2>&1
  code=$?
  set -e
  cat server.log
  [[ $code -eq 0 ]] || fail "hosting starter game '$name' exited with $code"
  grep -q "Nobody has played for" server.log || fail "hosting '$name' didn't stop on its own"
  if grep -qiE "panicked|script error" server.log; then fail "hosting '$name' printed an error"; fi
done <<< "$names"

if [[ "$mode" == "window" ]]; then
  echo "== the app's window, for $seconds seconds"
  run=("$app")
  if [[ "$(uname)" == "Linux" ]]; then
    run=(xvfb-run -a -s "-screen 0 1440x900x24" "$app")
    # Mesa's software renderer: the runner has no GPU.
    export WGPU_BACKEND=vulkan VK_ICD_FILENAMES=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json | head -1)
  fi
  set +e
  LIBROXEL_SMOKE_SECONDS="$seconds" timeout $((seconds + 240)) "${run[@]}" > window.log 2>&1
  code=$?
  set -e
  tail -n 60 window.log
  [[ $code -eq 0 ]] || fail "the app exited with $code instead of drawing for $seconds seconds"
  grep -q "smoke: drew" window.log || fail "the app never reported drawing"
fi
echo "Launch check passed."
