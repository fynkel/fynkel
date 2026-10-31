import 'dart:async';

import 'package:fynkel/fynkel.dart' show ViewStats;

/// When what was just spawned is first on screen, from the view's counts
/// after each frame ([FynkelView.onViewStats]).
///
/// When [next] is called, frames up to `posted` have been handed to the
/// engine without it, and the ones after have it. A later reading shows a
/// newer frame was drawn once more frames have been drawn since than
/// there were older ones still to draw. The next frame Flutter takes after
/// that is one of those or newer, so it shows the new objects. (On Android
/// a frame is given to the screen as it's drawn, so there this is up to a
/// frame late.)
class OnScreenClock {
  ViewStats _last = const ViewStats();

  // What each call of [next] still waits for, in order.
  final _waiting = <({ViewStats at, Stopwatch clock, Completer<Duration> done, int? presented})>[];

  /// Completes with [clock]'s time when the objects spawned until now are
  /// first on screen. Asked again before that, each wait completes when
  /// its own objects are.
  Future<Duration> next(Stopwatch clock) {
    final done = Completer<Duration>();
    _waiting.add((at: _last, clock: clock, done: done, presented: null));
    return done.future;
  }

  /// The view's counts after each frame it hands to the engine: give it
  /// as the view's [FynkelView.onViewStats].
  void onViewStats(ViewStats stats) {
    _last = stats;
    for (var i = 0; i < _waiting.length; i++) {
      final waiting = _waiting[i];
      final at = waiting.at;
      if (waiting.presented case final presented?) {
        if (stats.presented > presented) {
          _waiting.removeAt(i--);
          waiting.done.complete(waiting.clock.elapsed);
        }
        continue;
      }
      final olderLeft = at.posted - (at.rendered + at.coalesced + at.skipped);
      if (stats.rendered - at.rendered > olderLeft) {
        _waiting[i] = (at: at, clock: waiting.clock, done: waiting.done, presented: stats.presented);
      }
    }
  }
}
