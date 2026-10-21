# Pocket Hole

An original portrait game built with Fynkel: drag a hole around a floating board and collect every colored block. Six sculptures per level, four deterministic layout patterns, unlimited level progression, saved unlocks, pause/resume, restart, optional completion haptics, and custom app icons. No ads, analytics, account, or network gameplay.

## Run

From this directory:

```sh
flutter pub get
flutter run -d <device-id>
flutter test
flutter analyze
```

Android requires API 30+, the Android NDK and an arm64 device. iOS uses the engine's native Metal renderer. macOS is included for development. Web is not supported by Fynkel.

## Implementation

`lib/game.dart` owns the Fynkel scene and deterministic gameplay; `lib/main.dart` owns Flutter UI, touch input, lifecycle handling, and SharedPreferences persistence. Blocks are Rapier rigid bodies with box colliders, gravity, friction, continuous collision detection, and interpolated rendering. A 64-segment triangle ring forms the moving floor collider with a real circular opening; fixed perimeter colliders keep blocks on the board. The floor moves as a static collider so it removes support without dragging the entire board like a conveyor. Blocks near the swept opening wake while it moves. The hole follows drag input directly, with no follow easing; gravity is tuned to 24 m/s² and contact friction to 0.28 for quicker arcade-style collapses. Collection occurs only after a body falls below the board. Physics runs at 120 Hz and pauses with the game. The visible hole remains a dark surface disc over the decorative board; the circular opening is in the collision mesh. There are four layout variants; levels repeat those variants. There are no hazards or monetization systems.

## Prepare a store release

This example is playable source, not an already approved store release. Before submission:

1. Choose your own application ID and iOS bundle ID in the platform projects. Check your desired game name is available.
2. Android: create an upload keystore and an untracked `android/key.properties` containing `storeFile` (absolute path), `storePassword`, `keyAlias`, and `keyPassword`. Release signing uses these properties; it never falls back to debug signing. Run `flutter build appbundle --release --target-platform android-arm64`.
3. iOS: select your Apple developer team in Xcode, configure distribution signing, then run `flutter build ipa --release`. Set your own team rather than relying on a locally generated development setting.
4. Test on physical Android and iPhone devices, including app background/resume, tablet layout, thermal performance, and offline launch. Simulator performance is not a device performance guarantee.
5. Supply your support contact, hosted privacy policy, screenshots, store description, age rating, and the stores' data declarations. This example stores level and haptics preferences locally and intentionally includes no tracking SDKs.
6. Review the generated native project settings and current store requirements, increment `version` in `pubspec.yaml` for updates, and upload through your own store accounts.

`store-icon.png` is the original 1024px icon source. All game geometry and icons are generated specifically for this example; no assets from the reference game are used.
