# Fynkel Flutter macOS host

The Flutter game host (blueprint §5.2, ADR-0007/0008): Dart gameplay on
`package:fynkel` drives `step()` from the Flutter frame callback; the world's
extract is rendered by `platform/macos` into an IOSurface ring that a Flutter
`Texture` presents (Path A, ADR-0002). Flutter widgets are composited over
the 3D world in the same frame.

Run:

```sh
cd hosts/flutter_macos
flutter run -d macos --release
```

No library path: `package:fynkel`'s native-assets build hook (ADR-0015,
`packages/fynkel/hook/build.dart`) provides the one dylib that exports both
the engine ABI (`fynkel_*`) and the viewport ABI (`fynkel_macos_*`) as the
code asset `package:fynkel/fynkel_native`; inside this checkout it runs
`cargo build --release -p fynkel_platform_macos` for every architecture
Flutter targets (universal macOS builds need
`rustup target add x86_64-apple-darwin`). The bindings on both sides are
`@Native` externals on that asset.

`FYNKEL_HOST_MODE=none` runs the app without a viewport (raster control).
