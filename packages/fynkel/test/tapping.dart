// Made-up taps for the tests.

import 'package:fynkel/fynkel.dart';

var _nextFinger = 1;

/// Feeds [world]'s input a tap at [at], down and up, for its next update.
/// Its view is [at]'s size.
void tap(World world, ViewPoint at) {
  final id = _nextFinger++;
  world.input.simulate
    ..resize(at.width, at.height)
    ..fingerDown(id, at.x, at.y)
    ..fingerUp(id, at.x, at.y);
}
