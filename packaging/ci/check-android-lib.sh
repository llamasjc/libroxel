#!/usr/bin/env bash
# Checks an Android library would load: check-android-lib.sh <lib.so> <machine> <ELF32|ELF64> <rust target>
#
# - built for the right processor (a 32-bit build must stay 32-bit);
# - every loadable segment aligned to 16 KB (Android 15+ devices with
#   16 KB pages refuse anything less), the libc++ we ship too;
# - exports the entry points GameActivity looks up, from the same
#   GameActivity version as the Java library the app ships;
# - no ELF TLS, text relocations or packed relocations the oldest
#   supported Android can't load;
# - needs only libraries the oldest supported Android (minSdk) has, plus
#   the libc++ we ship, and every symbol it needs is in one of those.
set -euo pipefail
lib="$1"; machine="$2"; class="$3"; target="$4"
ndk="$ANDROID_NDK_LATEST_HOME/toolchains/llvm/prebuilt/linux-x86_64"
readelf="$ndk/bin/llvm-readelf"
nm="$ndk/bin/llvm-nm"
fail() { echo "::error::$(basename "$lib") ($target): $1"; exit 1; }
gradle="$(dirname "$0")/../android/app/build.gradle.kts"
api=$(sed -n 's/.*minSdk *= *\([0-9]*\).*/\1/p' "$gradle")
[[ -n "$api" ]] || fail "no minSdk in build.gradle.kts"

[[ -f "$lib" ]] || fail "not built"
ls -l "$lib"
header=$("$readelf" -h "$lib")
grep -q "Class:.*$class" <<< "$header" || fail "not $class: $(grep Class: <<< "$header")"
grep -q "Machine:.*$machine" <<< "$header" || fail "not built for $machine: $(grep Machine: <<< "$header")"

case "$target" in
  aarch64-*) sysroot_arch=aarch64-linux-android ;;
  armv7-*) sysroot_arch=arm-linux-androideabi ;;
  x86_64-*) sysroot_arch=x86_64-linux-android ;;
  *) fail "unknown target" ;;
esac
cxx="$ndk/sysroot/usr/lib/$sysroot_arch/libc++_shared.so"

# Program headers: LOAD lines end with the alignment.
aligned() {
  local bad=""
  while read -r kind rest; do
    [[ "$kind" == "LOAD" ]] || continue
    align="${rest##* }"
    (( align >= 16384 )) || bad+="LOAD $rest"$'\n'
  done < <("$readelf" -lW "$1")
  [[ -z "$bad" ]] || fail "$(basename "$1") segments not aligned to 16 KB:
$bad"
}
aligned "$lib"
[[ "$class" == ELF32 ]] || aligned "$cxx"

headers=$("$readelf" -lW "$lib")
dynamic=$("$readelf" -dW "$lib")
if (( api < 29 )) && grep -qw TLS <<< "$headers"; then
  fail "has ELF TLS, which needs Android 10 (API 29)"
fi
grep -qw TEXTREL <<< "$dynamic" && fail "has text relocations"
if (( api < 30 )) && grep -q '(RELR)' <<< "$dynamic"; then
  fail "has RELR relocations, which need Android 11 (API 30)"
fi
if (( api < 28 )) && grep -q '(ANDROID_RELR)' <<< "$dynamic"; then
  fail "has RELR relocations, which need Android 9 (API 28)"
fi

symbols=$("$nm" -D --defined-only "$lib")
for entry in android_main GameActivity_onCreate; do
  grep -qw "$entry" <<< "$symbols" || fail "doesn't export $entry, so the app can't start"
done

java=$(sed -n 's/.*games-activity:\([0-9.]*\).*/\1/p' "$gradle")
lock="$(dirname "$0")/../../Cargo.lock"
crate=$(grep -A1 '^name = "android-activity"' "$lock" | sed -n 's/version = "\(.*\)"/\1/p')
header=$(ls "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/android-activity-"$crate"/android-games-sdk/game-activity/prefab-src/modules/game-activity/include/game-activity/GameActivity.h | head -1)
native=$(for part in MAJOR MINOR BUGFIX; do sed -n "s/#define GAMEACTIVITY_${part}_VERSION \([0-9]*\)/\1/p" "$header"; done | paste -sd.)
[[ "$java" == "$native" ]] || fail "games-activity $java in build.gradle.kts, but android-activity $crate is built for $native"

system="$ndk/sysroot/usr/lib/$sysroot_arch/$api"
[[ -d "$system" ]] || fail "the NDK has no API $api libraries at $system"
needed=$(sed -n 's/.*(NEEDED).*\[\(.*\)\].*/\1/p' <<< "$dynamic")
echo "needs: $(tr '\n' ' ' <<< "$needed")"
while IFS= read -r so; do
  [[ -z "$so" || "$so" == "libc++_shared.so" ]] && continue
  [[ -e "$system/$so" ]] || fail "needs $so, which API $api doesn't have"
done <<< "$needed"

names() { sed 's/@.*//' | LC_ALL=C sort -u; }
have=$(for so in "$system"/*.so "$cxx"; do
  "$nm" -D --defined-only "$so" 2>/dev/null | awk '{print $NF}'
done | names)
missing=$("$nm" -D --undefined-only "$lib" | awk '$1 == "U" {print $2}' | names | LC_ALL=C comm -23 - <(echo "$have"))
[[ -z "$missing" ]] || fail "needs symbols API $api doesn't have:
$missing"
echo "$(basename "$lib") ($target) would load."
