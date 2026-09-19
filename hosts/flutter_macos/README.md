# Fynkel Flutter macOS host

The Flutter game host (blueprint §5.2, ADR-0007/0008): Dart gameplay on
`package:fynkel` drives `step()` from the Flutter frame callback; the world's
extract is rendered by `platform/macos` into an IOSurface ring that a Flutter
`Texture` presents (Path A, ADR-0002). Flutter widgets are composited over
the 3D world in the same frame.

Run (after `cargo build --release -p fynkel_platform_macos`):

```sh
cd hosts/flutter_macos
flutter run -d macos --release \
  --dart-define=FYNKEL_NATIVE_LIB=$PWD/../../target/release/libfynkel_platform_macos.dylib
```

`FYNKEL_NATIVE_LIB` (env var or dart-define) names the one dylib that exports
both the engine ABI (`fynkel_*`) and the viewport ABI (`fynkel_macos_*`); the
native-assets hook (ADR-0015, M1-08) replaces this.

`FYNKEL_HOST_MODE=none` runs the app without a viewport (raster control).
