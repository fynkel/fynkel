# package:fynkel_flutter

The Flutter host for Fynkel games: the Path A viewport (ADR-0002) as a
plugin, the widget that shows it and the loop that drives the world. See
`docs/hosts.md` ("Flutter host") and `docs/design/dart-sdk.md` §21.1.

```dart
final game = Game.load(projectDir);                    // package:fynkel
final viewport = FynkelViewportController(assetCache: game.report.cacheDir, placeholders: false);

FynkelGameView(
  world: game.world,
  controller: viewport,
  onStats: (s) => ...,                                 // per-second FynkelFrameStats
  overlay: const Hud(),                                // ordinary widgets over the 3D world
)
```

- `FynkelGameView` — a `Ticker` runs `World.frame(dt, fixedUpdate:)` once per
  Flutter frame (every behaviour's `fixedUpdate` per planned tick, the native
  step, `update`/`lateUpdate`), then renders the world's extract into the
  viewport. Mutes while the app is hidden, clamps dt, re-primes its clock
  after a stall, reports `FynkelFrameStats`.
- `FynkelViewport` — the `Texture` widget sized from layout × device pixel
  ratio, with focus and pointer events (positions in viewport physical
  pixels).
- `FynkelViewportController` — one native viewport: IOSurface ring, resize,
  asset cache, placeholders, camera, stats, ordered teardown.
- `darwin/fynkel_flutter` — the Swift plugin for macOS and iOS
  (`FynkelFlutterPlugin`, `FynkelViewportTexture`), registered by Flutter's
  generated plugin registrant.

The native code (renderer, ring, engine ABI) is the one dylib
`package:fynkel`'s build hook provides (ADR-0015); this package adds no
native build of its own. `flutter test` runs the tests against that real
runtime with the texture channel mocked.
