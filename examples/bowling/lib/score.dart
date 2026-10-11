// Ten-pin bowling's score: ten frames of two rolls, a strike (all ten with
// the first) counts the next two rolls too, a spare (all ten with two) the
// next one, and the tenth frame gives the rolls those need.

import 'package:flutter/foundation.dart' show ChangeNotifier;

class Score extends ChangeNotifier {
  final _frames = <List<int>>[[]];

  /// The frame being bowled, 0 to 9 (9 once the game is over too).
  int get frame => _frames.length - 1;

  /// Each frame's rolls so far.
  List<List<int>> get frames => _frames;

  bool get isOver {
    final tenth = _frames.length == 10 ? _frames.last : null;
    if (tenth == null) return false;
    return tenth.length == 3 || (tenth.length == 2 && tenth[0] + tenth[1] < 10);
  }

  /// [pins] more knocked down by a roll.
  void add(int pins) {
    if (isOver) throw StateError('the game is over');
    final rolls = _frames.last;
    rolls.add(pins);
    final done = frame < 9 && (rolls[0] == 10 || rolls.length == 2);
    if (done) _frames.add([]);
    notifyListeners();
  }

  /// Starts a new game.
  void clear() {
    _frames
      ..clear()
      ..add([]);
    notifyListeners();
  }

  /// Each frame's running total, or null while it waits for rolls to come.
  List<int?> get totals {
    final rolls = [for (final f in _frames) ...f];
    final out = <int?>[];
    var at = 0, sum = 0;
    for (final f in _frames) {
      if (f.isEmpty) break;
      final strike = f[0] == 10, spare = !strike && f.length >= 2 && f[0] + f[1] == 10;
      final needs = strike || spare ? 3 : 2;
      if (at + needs > rolls.length || (!strike && !spare && f.length < 2)) {
        out.add(null);
      } else {
        sum += rolls.sublist(at, at + needs).fold(0, (a, b) => a + b);
        out.add(sum);
      }
      at += f.length;
    }
    return out;
  }

  /// The score so far: the last total that is known.
  int get total => totals.lastWhere((t) => t != null, orElse: () => 0) ?? 0;

  /// What a frame's boxes show: X for a strike, / for a spare, - for none.
  List<String> marks(int frame) {
    if (frame >= _frames.length) return const [];
    final rolls = _frames[frame];
    final out = <String>[];
    for (var i = 0; i < rolls.length; i++) {
      // A strike, or the ten after one in the tenth frame.
      final fresh = i == 0 || (frame == 9 && (rolls[i - 1] == 10 || (i == 2 && rolls[0] + rolls[1] == 10)));
      if (fresh && rolls[i] == 10) {
        out.add('X');
      } else if (!fresh && rolls[i - 1] + rolls[i] == 10) {
        out.add('/');
      } else {
        out.add(rolls[i] == 0 ? '-' : '${rolls[i]}');
      }
    }
    return out;
  }

  /// The last roll's mark.
  String get lastMark {
    for (var f = _frames.length - 1; f >= 0; f--) {
      final m = marks(f);
      if (m.isNotEmpty) return m.last;
    }
    return '';
  }
}
