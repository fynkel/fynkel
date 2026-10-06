#!/bin/sh
# Rebuilds the Bevy comparison app (run C3) for an iPhone and prints the
# signed .app's path. Does not install or launch anything.
#
#   benchmarks/compare/bevy/build_ios.sh          # device (aarch64-apple-ios)
#   benchmarks/compare/bevy/build_ios.sh --sim    # iOS Simulator (arm64)
#
# Install / launch (by hand):
#   xcrun devicectl device install app --device <UDID> <printed .app>
#   xcrun devicectl device process launch --device <UDID> --console \
#     --environment-variables '{"CMP_LEVEL":"full","CMP_SCALE":"1.0"}' \
#     dev.fynkel.compare.bevy
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"

if [ "${1:-}" = "--sim" ]; then
  RUST_TARGET=aarch64-apple-ios-sim; SDK=iphonesimulator
  DEST="generic/platform=iOS Simulator"; SIGN=""
else
  RUST_TARGET=aarch64-apple-ios; SDK=iphoneos
  DEST='generic/platform=iOS'; SIGN=""
fi

# rust-toolchain.toml here pins the toolchain (Bevy 0.19.1 needs >= 1.95).
IPHONEOS_DEPLOYMENT_TARGET=16.0 cargo build --release --lib --target "$RUST_TARGET" >&2

# shellcheck disable=SC2086
xcodebuild -project ios/CmpBevy.xcodeproj -scheme CmpBevy -configuration Release \
  -sdk "$SDK" -destination "$DEST" -derivedDataPath ios/build \
  -allowProvisioningUpdates $SIGN build >&2

APP="$HERE/ios/build/Build/Products/Release-$SDK/CmpBevy.app"
test -d "$APP"
echo "$APP"
