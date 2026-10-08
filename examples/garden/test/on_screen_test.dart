import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show ViewStats;
import 'package:garden/on_screen.dart';

void main() {
  ViewStats at(int posted, int rendered, {int coalesced = 0, int presented = 0}) =>
      ViewStats(posted: posted, rendered: rendered, coalesced: coalesced, presented: presented);

  /// Feeds [readings] one by one; the index of the one that completed the
  /// wait, or null.
  Future<int?> shownAt(ViewStats spawnedAfter, List<ViewStats> readings) async {
    final clock = OnScreenClock()..onViewStats(spawnedAfter);
    int? index;
    var reading = 0;
    clock.next(Stopwatch()).then((_) => index = reading);
    for (; reading < readings.length; reading++) {
      clock.onViewStats(readings[reading]);
      await Future<void>.delayed(Duration.zero);
      if (index != null) break;
    }
    return index;
  }

  test('on screen once a frame posted after the spawn is drawn, and then taken', () async {
    // Spawned after frame 10 was posted; 9 and 10 were still to draw.
    final readings = [
      at(11, 9, presented: 8), // 9 drawn
      at(12, 10, presented: 9), // 10 drawn: still older than the spawn
      at(13, 11, presented: 10), // 11 drawn: the first with it
      at(14, 12, presented: 10), // not taken yet
      at(15, 13, presented: 11), // taken: 11 or newer
    ];
    expect(await shownAt(at(10, 8, presented: 7), readings), 4);
  });

  test('never early: frames that were replaced before they were drawn may be the old ones', () async {
    // 9 and 10 still to draw. 9 is replaced by 10, then 10 drawn: one
    // drawn frame, and it could be 10, so nothing is known yet. Then 11
    // replaced by 12, which is drawn: that's new.
    final readings = [
      at(11, 9, coalesced: 1, presented: 8),
      at(12, 9, coalesced: 2, presented: 9),
      at(13, 10, coalesced: 2, presented: 9),
      at(14, 11, coalesced: 2, presented: 10),
      at(15, 12, coalesced: 2, presented: 11),
    ];
    expect(await shownAt(at(10, 8, presented: 7), readings), 4);
  });

  test('a view with no frames yet: its first drawn frame has everything', () async {
    final readings = [at(1, 0), at(2, 1), at(3, 2, presented: 1)];
    expect(await shownAt(const ViewStats(), readings), 2);
  });

  test('a hidden view never shows it', () async {
    final readings = [for (var i = 1; i < 20; i++) ViewStats(posted: 10 + i, rendered: 8, skipped: i)];
    expect(await shownAt(at(10, 8), readings), isNull);
  });
}
