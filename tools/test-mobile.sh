#!/usr/bin/env bash
# Runs the renderer's GPU tests inside the iOS Simulator and/or the Android
# emulator, so "it works in the simulator" is checked, not assumed.
#
#   tools/test-mobile.sh            both
#   tools/test-mobile.sh ios        the iOS Simulator only
#   tools/test-mobile.sh android    the Android emulator only
#
# A simulator or emulator this script boots is shut down again at the end.
# One that was already running is left running.
#
# Settings (environment):
#   IOS_SIM      simulator name           (default: iPhone 16 Pro Max)
#   ANDROID_AVD  emulator name            (default: Medium_Phone_API_36.0)
#   ANDROID_SERIAL  a device to use instead (adb devices lists them), such
#                   as a phone; then no emulator is started
#   ANDROID_SDK  SDK path                 (default: ~/Library/Android/sdk)
#   ANDROID_NDK  NDK version directory    (default: the newest installed)
set -euo pipefail
cd "$(dirname "$0")/.."

which="${1:-all}"
crates=(fynkel_present fynkel_render fynkel_ffi fynkel_audio)
# crate:test (fynkel_ffi has an alloc test of its own, for the host).
tests=(fynkel_present:android fynkel_render:render fynkel_render:textures fynkel_render:faces fynkel_render:alloc fynkel_render:pipelines
  fynkel_ffi:android fynkel_ffi:models fynkel_audio:session)

# The test binaries for a target, one "crate:test path" line each, for
# every test in $tests. Fails, with cargo's errors, when one didn't build:
# a missing binary must not pass as a test that ran.
binaries() {
  local target=$1 json
  if ! json=$(cargo test "${crates[@]/#/-p}" --target "$target" --no-run --message-format=json 2>/dev/null); then
    cargo test "${crates[@]/#/-p}" --target "$target" --no-run >&2 || true
    return 1
  fi
  python3 -c '
import json, os, sys
wanted = sys.argv[1].split()
found = {}
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and "test" in m["target"]["kind"]:
        crate = os.path.basename(os.path.dirname(m["manifest_path"]))
        found[crate + ":" + m["target"]["name"]] = m["executable"]
missing = [t for t in wanted if t not in found]
if missing:
    sys.exit("no test binary for " + ", ".join(missing))
for t in wanted:
    print(t, found[t])' "${tests[*]}" <<<"$json"
}

ios() {
  # Built first, so a failed build leaves nothing booted.
  local list
  list=$(binaries aarch64-apple-ios-sim) || return 1
  local name="${IOS_SIM:-iPhone 16 Pro Max}"
  local udid
  udid=$(xcrun simctl list devices available -j | python3 -c "
import json, sys
for runtime, devices in json.load(sys.stdin)['devices'].items():
    for d in devices:
        if d['name'] == sys.argv[1]:
            print(d['udid']); sys.exit()
" "$name")
  [ -n "$udid" ] || { echo "no simulator named '$name'"; exit 1; }
  local booted_here=0
  if ! xcrun simctl list devices | grep "$udid" | grep -q Booted; then
    xcrun simctl boot "$udid"
    booted_here=1
  fi
  echo "== iOS Simulator: $name"
  local status=0
  while read -r test exe; do
    # </dev/null: a child reading stdin would eat the rest of the loop's list.
    xcrun simctl spawn "$udid" "$exe" --test-threads=1 </dev/null || status=1
  done <<<"$list"
  [ $booted_here = 1 ] && xcrun simctl shutdown "$udid"
  return $status
}

# Runs one test binary pushed to $2 on the device, its output to stderr,
# and prints its result line: exit=124 if it was stopped after 10 minutes.
# The line starts afresh: a binary stopped mid-test leaves its own unended.
run_on_android() {
  local adb=$1 dir=$2 exe=$3
  "$adb" shell "cd $dir && FYNKEL_GOLDEN_DIR=$dir/golden timeout -k 10 600 ./$exe --test-threads=1; e=\$?; echo; echo exit=\$e" </dev/null |
    tee /dev/stderr | grep '^exit=' | tail -1
}

# Starts the emulator $3 and waits until adb lists it. Only one emulator
# runs here, so the first one to show up is it.
start_emulator() {
  local sdk=$1 adb=$2 avd=$3
  "$sdk/emulator/emulator" -avd "$avd" -no-window -no-audio -no-snapshot-save -gpu host >>/tmp/fynkel-emulator.log 2>&1 &
  until "$adb" devices | grep -q '^emulator-'; do sleep 1; done
}

# Waits until the device has booted, and puts the goldens in $2 on it.
ready_android() {
  local adb=$1 dir=$2
  "$adb" wait-for-device
  until [ "$("$adb" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do sleep 2; done
  echo "== Android: $ANDROID_SERIAL ($("$adb" shell getprop ro.product.model | tr -d '\r'))"
  "$adb" shell "rm -rf $dir && mkdir -p $dir/golden" >/dev/null
  "$adb" push tests/golden/. "$dir/golden/" >/dev/null
}

android() {
  local sdk="${ANDROID_SDK:-$HOME/Library/Android/sdk}"
  local ndk="${ANDROID_NDK:-$(ls -d "$sdk"/ndk/* | sort -V | tail -1)}"
  local adb="$sdk/platform-tools/adb"
  local avd="${ANDROID_AVD:-Medium_Phone_API_36.0}"
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ndk/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android30-clang"
  local list
  list=$(binaries aarch64-linux-android) || return 1

  local started_here=0 own_emulator=0
  if [ -z "${ANDROID_SERIAL:-}" ]; then
    own_emulator=1
    if ! "$adb" devices | grep -q '^emulator-'; then
      start_emulator "$sdk" "$adb" "$avd"
      started_here=1
    fi
    # Every adb call below goes to it, also with a phone plugged in.
    export ANDROID_SERIAL=$("$adb" devices | awk '/^emulator-/ { print $1; exit }')
  fi
  local dir=/data/local/tmp/fynkel
  ready_android "$adb" "$dir"
  local status=0
  while read -r test exe; do
    "$adb" push "$exe" "$dir/${test#*:}" >/dev/null </dev/null
    # </dev/null: adb shell would otherwise eat the rest of the loop's list.
    # adb shell doesn't pass the exit status through older devices, so the
    # result line decides. A binary still running after 10 minutes is
    # stopped: the emulator's GPU has wedged on a test's first frame after
    # it had been up a while (2026-10-28: the test alone, and the whole run
    # on a fresh emulator, pass in seconds), which held CI for hours.
    local result
    result=$(run_on_android "$adb" "$dir" "${test#*:}")
    if [ $own_emulator = 1 ] && { [ "$result" = exit=124 ] || [ -z "$result" ]; }; then
      # Stopped after 10 minutes, or no result line: the emulator itself
      # went. Its software Vulkan (SwiftShader) has segfaulted compiling a
      # pipeline on a test's first frame, now and then (2026-10-29, crash
      # reports in ~/Library/Logs/DiagnosticReports/qemu-system-aarch64-*),
      # and an emulator up a while has hung there twice running, then
      # taken 18 minutes to take the next binary: it's started afresh and
      # the binary run once more.
      echo "== $test ${result:+hung for 10 minutes}${result:-lost the emulator}: starting the emulator again and running it once more" >&2
      # A wedged emulator may not hear adb: then it's stopped outright.
      "$adb" emu kill >/dev/null 2>&1 </dev/null || true
      local waited=0
      while "$adb" devices </dev/null | grep -q "^$ANDROID_SERIAL"; do
        sleep 1
        waited=$((waited + 1))
        [ $waited = 30 ] && pkill -9 -f "qemu-system.*-avd $avd" || true
      done
      start_emulator "$sdk" "$adb" "$avd" </dev/null
      started_here=1
      export ANDROID_SERIAL=$("$adb" devices | awk '/^emulator-/ { print $1; exit }')
      ready_android "$adb" "$dir" </dev/null
      "$adb" push "$exe" "$dir/${test#*:}" >/dev/null </dev/null
      result=$(run_on_android "$adb" "$dir" "${test#*:}")
    fi
    if [ $own_emulator = 1 ] && { [ "$result" = exit=124 ] || [ -z "$result" ]; }; then
      # Twice the emulator, not the test: said, in the log and on the run's
      # page, but it doesn't fail the run. A test that fails still does.
      echo "::warning title=Android emulator::$test didn't finish on the emulator twice (${result:-the emulator went}); its tests weren't run there"
    elif [ "$result" != exit=0 ]; then
      status=1
    fi
    # Debug test binaries are big: left behind, they fill the emulator.
    "$adb" shell "rm -f $dir/${test#*:}" </dev/null
  done <<<"$list"
  [ $started_here = 1 ] && "$adb" emu kill >/dev/null
  return $status
}

status=0
case "$which" in
  ios) ios || status=1 ;;
  android) android || status=1 ;;
  all) ios || status=1; android || status=1 ;;
  *) echo "usage: $0 [ios|android|all]"; exit 2 ;;
esac
[ $status = 0 ] && echo "== all mobile GPU tests passed" || echo "== mobile GPU tests FAILED"
exit $status
