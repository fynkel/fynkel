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
  ({ViewStats at, Stopwatch clock, Completer<Duration> done, int? presented})? _waiting;

  /// Completes with [clock]'s time when the objects spawned until now are
  /// first on screen.
  Future<Duration> next(Stopwatch clock) {
    final done = Completer<Duration>();
    _waiting = (at: _last, clock: clock, done: done, presented: null);
    return done.future;
  }

  void onViewStats(ViewStats stats) {
    _last = stats;
    final waiting = _waiting;
    if (waiting == null) return;
    final at = waiting.at;
    if (waiting.presented case final presented?) {
      if (stats.presented > presented) {
        _waiting = null;
        waiting.done.complete(waiting.clock.elapsed);
      }
      return;
    }
    final olderLeft = at.posted - (at.rendered + at.coalesced + at.skipped);
    if (stats.rendered - at.rendered > olderLeft) {
      _waiting = (at: at, clock: waiting.clock, done: waiting.done, presented: stats.presented);
    }
  }
}
