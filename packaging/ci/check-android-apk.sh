#!/usr/bin/env bash
# Checks a release APK: check-android-apk.sh <app.apk>
#
# - every public method of MainActivity survived R8, since native code
#   calls them by name;
# - each ABI carries both libraries, stored uncompressed at 16 KB offsets.
set -euo pipefail
apk="$1"
fail() { echo "::error::$(basename "$apk"): $1"; exit 1; }
[[ -f "$apk" ]] || fail "not built"

java="$(dirname "$0")/../android/app/src/main/java/com/libroxel/app/MainActivity.java"
strings=$(unzip -p "$apk" 'classes*.dex' | strings -a)
for method in $(grep -oP 'public \S+ \K\w+(?=\()' "$java"); do
  grep -qx "$method" <<< "$strings" || fail "R8 removed MainActivity.$method"
done

python3 -I - "$apk" <<'PY' || exit 1
import struct, sys, zipfile
apk = sys.argv[1]
z = zipfile.ZipFile(apk)
names = set(z.namelist())
bad = []
for abi in ("arm64-v8a", "armeabi-v7a"):
    for lib in ("libplaylibre.so", "libc++_shared.so"):
        if f"lib/{abi}/{lib}" not in names:
            bad.append(f"no lib/{abi}/{lib}")
with open(apk, "rb") as f:
    for info in z.infolist():
        if not info.filename.endswith(".so"):
            continue
        f.seek(info.header_offset)
        name_len, extra_len = struct.unpack("<HH", f.read(30)[26:30])
        offset = info.header_offset + 30 + name_len + extra_len
        if info.compress_type != zipfile.ZIP_STORED:
            bad.append(f"{info.filename} is compressed")
        elif offset % 16384:
            bad.append(f"{info.filename} not at a 16 KB offset")
for line in bad:
    print(f"::error::{apk}: {line}")
sys.exit(1 if bad else 0)
PY
echo "$(basename "$apk") is fine."
