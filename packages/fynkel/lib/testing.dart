/// For a game's tests: run its world for a while, tap it, and name a point
/// in its view. `HeadlessView` draws it, and `view.png()` saves a frame.
///
/// ```dart
/// final world = World()..input.simulate.resize(320, 200);
/// final game = world.add(MyGame());
/// world.run(2);
/// world.tap(160, 100);
/// world.run(0.1);
/// ```
library;

import 'fynkel.dart';

/// [run], [tap] and [viewPoint] on a [World], for tests.
extension WorldTesting on World {
  /// Runs [seconds] of frames, [fps] a second: `world.update(1 / fps)`
  /// that many times.
  void run(double seconds, {int fps = 60}) {
    final frames = (seconds * fps).round();
    for (var i = 0; i < frames; i++) {
      update(1 / fps);
    }
  }

  /// A tap at ([x], [y]) in the view's logical pixels, on the next frame:
  /// a finger down and up again, a new one each time. Throws a
  /// [StateError] while the input has no view size.
  void tap(double x, double y) {
    _sized('tap');
    final id = _fingers[this] = (_fingers[this] ?? 1 << 20) + 1;
    input.simulate
      ..fingerDown(id, x, y)
      ..fingerUp(id, x, y);
  }

  /// ([x], [y]) in the view, as the input gives a finger's place: for
  /// `camera.screenPointToRay` and the like. Throws a [StateError] while
  /// the input has no view size.
  ViewPoint viewPoint(double x, double y) {
    _sized('viewPoint');
    return ViewPoint(x, y, width: input.width, height: input.height);
  }

  void _sized(String what) {
    if (input.width > 0 && input.height > 0) return;
    throw StateError('$what needs a view size: give one with world.input.simulate.resize(width, height)');
  }
}

// The last finger id each world's taps used: far above a real pointer's.
final _fingers = Expando<int>('tap fingers');
