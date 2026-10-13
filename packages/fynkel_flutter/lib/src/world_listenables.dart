import 'package:flutter/foundation.dart';
import 'package:fynkel/fynkel.dart' show World;

/// A [World]'s state as Flutter listenables, for widgets beside the game.
extension WorldListenables on World {
  /// [World.paused] as a [ValueListenable], for a pause panel that shows
  /// itself: `ValueListenableBuilder(valueListenable:
  /// game.world.pausedListenable, ...)`. The same one each time it's asked
  /// for.
  ValueListenable<bool> get pausedListenable => _paused[this] ??= _pausedOf(this);
}

final _paused = Expando<ValueNotifier<bool>>('pausedListenable');

ValueNotifier<bool> _pausedOf(World world) {
  final notifier = ValueNotifier(world.paused);
  world.onPausedChanged.listen((paused) => notifier.value = paused);
  return notifier;
}
