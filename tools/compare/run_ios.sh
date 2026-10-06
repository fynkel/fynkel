#!/bin/sh
# One measured run on the iPhone (docs/engine-comparison.md):
#   tools/compare/run_ios.sh <fynkel|bevy|unity> <plain|full|msaa> <scale> <seconds>
# Launches the installed app with CMP_LEVEL/CMP_SCALE, keeps its console for
# <seconds>, and writes benchmarks/compare/results/<engine>_<level>_<scale>.log.
set -eu
engine=$1 level=$2 scale=$3 secs=$4
device=${CMP_DEVICE:-00008140-0014710C3EFB001C}
case $engine in
  fynkel) bundle=com.example.compareFynkel ;;
  bevy) bundle=dev.fynkel.compare.bevy ;;
  unity) bundle=dev.fynkel.compare.unity ;;
  *) echo "unknown engine $engine" >&2; exit 2 ;;
esac
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/benchmarks/compare/results/${engine}_${level}_${scale}.log"
xcrun devicectl device process launch --device "$device" --console --terminate-existing \
  --environment-variables "{\"CMP_LEVEL\":\"$level\",\"CMP_SCALE\":\"$scale\"}" "$bundle" > "$out" 2>&1 &
pid=$!
sleep "$secs"
kill "$pid" 2>/dev/null || true
python3 "$root/tools/compare/summary.py" "$out"
