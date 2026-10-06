#!/bin/bash
# Engine comparison, run C4 (docs/engine-comparison.md): Unity 6 URP iOS app.
#
#   benchmarks/compare/unity/build_ios.sh
#
# 1. Unity (batch mode) imports the assets, writes the URP/scene setup
#    (CmpSetup.Setup) and exports the Xcode project to Build/ios.
# 2. xcodebuild builds it, Release, signed (team N2VZ98KJRE, automatic).
# Prints the .app path on the last line. Does not install or launch.
#
# Launch (CMP_LEVEL plain|full|msaa, CMP_SCALE 1.0|0.75, CMP_SHOT=1):
#   xcrun devicectl device install app --device <id> <app>
#   xcrun devicectl device process launch --device <id> --console \
#     --environment-variables '{"CMP_LEVEL":"full","CMP_SCALE":"1.0"}' \
#     dev.fynkel.compare.unity
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
UNITY="${UNITY:-/Applications/Unity/Hub/Editor/6000.3.9f1/Unity.app/Contents/MacOS/Unity}"
TEAM="N2VZ98KJRE"
mkdir -p "$HERE/Build"

echo "== Unity: export Xcode project (log: Build/unity_ios.log)" >&2
"$UNITY" -batchmode -nographics -projectPath "$HERE" -buildTarget iOS \
  -executeMethod CmpSetup.BuildIOS -cmpOut "$HERE/Build/ios" \
  -quit -logFile "$HERE/Build/unity_ios.log" || {
  echo "Unity build failed; tail of Build/unity_ios.log:" >&2
  tail -40 "$HERE/Build/unity_ios.log" >&2
  exit 1
}
grep -q 'IL2CPP_CONFIG=\\"ReleasePlus\\"' "$HERE/Build/ios/Unity-iPhone.xcodeproj/project.pbxproj" \
  || echo "warning: IL2CPP ReleasePlus (Master) patch missing" >&2

echo "== xcodebuild (log: Build/xcodebuild_ios.log)" >&2
xcodebuild \
  -project "$HERE/Build/ios/Unity-iPhone.xcodeproj" \
  -scheme Unity-iPhone \
  -configuration Release \
  -sdk iphoneos \
  -destination 'generic/platform=iOS' \
  -derivedDataPath "$HERE/Build/ios_dd" \
  -allowProvisioningUpdates \
  DEVELOPMENT_TEAM="$TEAM" CODE_SIGN_STYLE=Automatic \
  build > "$HERE/Build/xcodebuild_ios.log" 2>&1 || {
  echo "xcodebuild failed; errors from Build/xcodebuild_ios.log:" >&2
  grep -E "error:|BUILD FAILED" "$HERE/Build/xcodebuild_ios.log" | head -40 >&2
  exit 1
}

APP="$HERE/Build/ios_dd/Build/Products/Release-iphoneos/CmpUnity.app"
[ -d "$APP" ] || { echo "no app at $APP" >&2; exit 1; }
codesign -dv "$APP" 2>&1 | grep -E "Identifier|TeamIdentifier" >&2 || true
echo "$APP"
