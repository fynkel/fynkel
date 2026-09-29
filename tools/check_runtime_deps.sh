#!/bin/sh
# The runtime a game links (fynkel_native without `authoring`, every
# module on) carries no document validation (ADR-0041): the cook checked
# every document, so JSON Schema and the text diffs stay in the tools.
set -eu
cd "$(dirname "$0")/.."
tree=$(cargo tree -p fynkel_native --no-default-features \
  --features physics,audio,animation -e normal --prefix none)
bad=0
for crate in jsonschema similar; do
  if printf '%s\n' "$tree" | grep -q "^$crate v"; then
    echo "the game runtime links $crate:"
    cargo tree -p fynkel_native --no-default-features \
      --features physics,audio,animation -e normal -i "$crate" | head -12
    bad=1
  fi
done
exit $bad
