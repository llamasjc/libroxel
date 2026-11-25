#!/usr/bin/env bash
# Checks an Android library would load: check-android-lib.sh <lib.so> <machine> <ELF32|ELF64> <rust target>
#
# - built for the right processor (a 32-bit build must stay 32-bit);
# - every loadable segment aligned to 16 KB (Android 15+ devices with
#   16 KB pages refuse anything less);
# - exports the entry points GameActivity looks up;
# - needs only libraries Android 8 (API 26) has, plus the libc++ we ship;
# - every symbol it needs is in one of those.
set -euo pipefail
lib="$1"; machine="$2"; class="$3"; target="$4"
ndk="$ANDROID_NDK_LATEST_HOME/toolchains/llvm/prebuilt/linux-x86_64"
readelf="$ndk/bin/llvm-readelf"
nm="$ndk/bin/llvm-nm"
fail() { echo "::error::$(basename "$lib") ($target): $1"; exit 1; }

[[ -f "$lib" ]] || fail "not built"
ls -l "$lib"
header=$("$readelf" -h "$lib")
grep -q "Class:.*$class" <<< "$header" || fail "not $class: $(grep Class: <<< "$header")"
grep -q "Machine:.*$machine" <<< "$header" || fail "not built for $machine: $(grep Machine: <<< "$header")"

# Program headers: LOAD lines end with the alignment.
bad=""
while read -r kind rest; do
  [[ "$kind" == "LOAD" ]] || continue
  align="${rest##* }"
  (( align >= 16384 )) || bad+="LOAD $rest"$'\n'
done < <("$readelf" -lW "$lib")
[[ -z "$bad" ]] || fail "segments not aligned to 16 KB:
$bad"

symbols=$("$nm" -D --defined-only "$lib")
for entry in android_main GameActivity_onCreate; do
  grep -qw "$entry" <<< "$symbols" || fail "doesn't export $entry, so the app can't start"
done

case "$target" in
  aarch64-*) sysroot_arch=aarch64-linux-android ;;
  armv7-*) sysroot_arch=arm-linux-androideabi ;;
  x86_64-*) sysroot_arch=x86_64-linux-android ;;
  *) fail "unknown target" ;;
esac
system="$ndk/sysroot/usr/lib/$sysroot_arch/26"
[[ -d "$system" ]] || fail "the NDK has no API 26 libraries at $system"
needed=$("$readelf" -dW "$lib" | sed -n 's/.*(NEEDED).*\[\(.*\)\].*/\1/p')
echo "needs: $(tr '\n' ' ' <<< "$needed")"
while IFS= read -r so; do
  [[ -z "$so" || "$so" == "libc++_shared.so" ]] && continue
  [[ -e "$system/$so" ]] || fail "needs $so, which Android 8 doesn't have"
done <<< "$needed"

names() { sed 's/@.*//' | LC_ALL=C sort -u; }
have=$(for so in "$system"/*.so "$ndk/sysroot/usr/lib/$sysroot_arch/libc++_shared.so"; do
  "$nm" -D --defined-only "$so" 2>/dev/null | awk '{print $NF}'
done | names)
missing=$("$nm" -D --undefined-only "$lib" | awk '$1 == "U" {print $2}' | names | LC_ALL=C comm -23 - <(echo "$have"))
[[ -z "$missing" ]] || fail "needs symbols Android 8 doesn't have:
$missing"
echo "$(basename "$lib") ($target) would load."
