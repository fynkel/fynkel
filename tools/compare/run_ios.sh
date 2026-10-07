#!/bin/sh
# One measured run on the iPhone (docs/engine-comparison.md):
#   tools/compare/run_ios.sh <fynkel|bevy|unity|spike> <plain|full|msaa> <scale> <seconds> [scene]
# Launches the installed app with CMP_LEVEL/CMP_SCALE (and CMP_SCENE),
# keeps its console for <seconds>, and writes
# benchmarks/compare/results/<engine>[_<scene>]_<level>_<scale>.log.
# The Fynkel app's release `print` does not reach the console over a
# wireless connection, so its [cmp] lines are copied from the app's
# tmp/cmp.log afterwards and appended.
set -eu
engine=$1 level=$2 scale=$3 secs=$4 scene=${5:-}
device=${CMP_DEVICE:-00008140-0014710C3EFB001C}
case $engine in
  fynkel) bundle=com.example.compareFynkel ;;
  bevy) bundle=dev.fynkel.compare.bevy ;;
  unity) bundle=dev.fynkel.compare.unity ;;
  *) echo "unknown engine $engine" >&2; exit 2 ;;
esac
root="$(cd "$(dirname "$0")/../.." && pwd)"
name=$engine${scene:+_$scene}
out="$root/benchmarks/compare/results/${name}_${level}_${scale}.log"
DEVICECTL_CHILD_CMP_LEVEL=$level DEVICECTL_CHILD_CMP_SCALE=$scale DEVICECTL_CHILD_CMP_SCENE=${scene:-sponza} \
xcrun devicectl device process launch --device "$device" --console --terminate-existing \
  --environment-variables "{\"CMP_LEVEL\":\"$level\",\"CMP_SCALE\":\"$scale\",\"CMP_SCENE\":\"${scene:-sponza}\"}" "$bundle" > "$out" 2>&1 &
pid=$!
sleep "$secs"
kill "$pid" 2>/dev/null || true
if [ "$engine" = fynkel ]; then
  tmp=$(mktemp)
  xcrun devicectl device copy from --device "$device" --domain-type appDataContainer \
    --domain-identifier "$bundle" --source tmp/cmp.log --destination "$tmp" > /dev/null 2>&1 && cat "$tmp" >> "$out"
  rm -f "$tmp"
fi
# Stop the app so the phone cools between runs (killing the console does
# not end it).
xcrun devicectl device info apps --device "$device" --bundle-id "$bundle" --json-output /tmp/cmp_apps.json > /dev/null 2>&1
xcrun devicectl device info processes --device "$device" --json-output /tmp/cmp_procs.json > /dev/null 2>&1
apppid=$(python3 -c '
import json
url = json.load(open("/tmp/cmp_apps.json"))["result"]["apps"][0]["url"]
for p in json.load(open("/tmp/cmp_procs.json"))["result"]["runningProcesses"]:
    if p.get("executable", "").startswith(url):
        print(p["processIdentifier"])
        break
')
[ -n "$apppid" ] && xcrun devicectl device process terminate --device "$device" --pid "$apppid" --kill > /dev/null 2>&1 || true
python3 "$root/tools/compare/summary.py" "$out"
