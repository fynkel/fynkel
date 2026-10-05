#!/bin/bash
# Opens a stress app for looking at it: tools/stress_visual.sh 5000 [old|new] [seconds]
# new = render worker + frame queue + 5 buffers (the game default); old = synchronous mailbox.
n=${1:-5000}; mode=${2:-new}; secs=${3:-60}
app="$(cd "$(dirname "$0")/.." && pwd)/target/mac-app-$n/build/macos/Build/Products/Release/fynkel_stress.app"
[ -d "$app" ] || { echo "build it first: cd target/mac-app-$n && flutter build macos --release"; exit 1; }
if [ "$mode" = new ]; then opts='"pipelined": true, "bufferCount": 5, "frameQueue": 2'; else opts='"pipelined": false'; fi
id=$(python3 -c "import secrets;print(secrets.token_hex(16))")
echo "{\"captureId\":\"$id\",$opts,\"limitSeconds\":$secs}" > ~/Documents/capture-request.json
open -n "$app"
