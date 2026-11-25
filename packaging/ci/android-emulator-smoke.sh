#!/usr/bin/env bash
# android-emulator-smoke.sh <app.apk>: installs the app on the running
# emulator, opens it and checks it is still alive after half a minute
# with no crash in the log.
set -euo pipefail
apk="$1"
package=com.libroxel.app
fail() { echo "::error::$1"; adb logcat -d -t 400 || true; exit 1; }

adb wait-for-device
adb install -r "$apk"
adb logcat -c
adb shell am start -W -n "$package/.MainActivity"
sleep 30
adb logcat -d > logcat.txt
grep -E "RustStdoutStderr|libroxel|$package" logcat.txt | tail -n 80 || true
pid=$(adb shell pidof "$package" | tr -d '\r' || true)
[[ -n "$pid" ]] || fail "the app isn't running 30 seconds after it was opened"
if grep -qE "FATAL EXCEPTION|panicked at|Fatal signal|UnsatisfiedLinkError" logcat.txt; then
  fail "the app crashed or failed to load"
fi
adb shell screencap -p /sdcard/smoke.png && adb pull /sdcard/smoke.png smoke.png || true
echo "The app opened and kept running (pid $pid)."
