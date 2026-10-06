#!/bin/sh
# Fetches Khronos' glTF Sponza into benchmarks/compare/sponza/ for the engine
# comparison (docs/engine-comparison.md). Not committed: the model is under
# the CRYENGINE Limited License Agreement (benchmarking only, no
# redistribution).
set -eu
dest="$(cd "$(dirname "$0")/../.." && pwd)/benchmarks/compare/sponza"
mkdir -p "$dest"
base="https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/main/Models/Sponza/glTF"
curl -fsSL "https://api.github.com/repos/KhronosGroup/glTF-Sample-Assets/contents/Models/Sponza/glTF" |
  python3 -c 'import json,sys; [print(f["name"]) for f in json.load(sys.stdin)]' |
  while read -r name; do
    [ -f "$dest/$name" ] || curl -fsSL -o "$dest/$name" "$base/$name"
  done
echo "Sponza: $(ls "$dest" | wc -l | tr -d ' ') files in $dest"
