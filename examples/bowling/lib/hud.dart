// What the game shows over the view, in plain Flutter: the score sheet,
// and a line under it.

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/widgets.dart';

import 'score.dart';

class Hud extends StatelessWidget {
  const Hud(this.score, this.message, {super.key});

  final Score score;
  final ValueListenable<String> message;

  static const _ink = Color(0xFFFFFFFF), _box = Color(0xCC1A1C24);

  @override
  Widget build(BuildContext context) => SafeArea(
    child: Padding(
      padding: const EdgeInsets.all(12),
      child: Column(
        children: [
          ListenableBuilder(listenable: score, builder: (context, _) => _sheet()),
          const SizedBox(height: 8),
          ValueListenableBuilder(
            valueListenable: message,
            builder: (context, message, _) => Text(
              message,
              style: const TextStyle(fontSize: 22, fontWeight: FontWeight.bold, color: _ink),
            ),
          ),
        ],
      ),
    ),
  );

  /// Ten boxes, each with its rolls' marks above and its total below.
  Widget _sheet() {
    final totals = score.totals;
    // Smaller on a narrow screen, not cut off.
    return FittedBox(
      fit: BoxFit.scaleDown,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (var f = 0; f < 10; f++)
            Container(
              width: f == 9 ? 58 : 36,
              margin: const EdgeInsets.all(1),
              padding: const EdgeInsets.symmetric(vertical: 3),
              decoration: BoxDecoration(
                color: _box,
                border: Border.all(color: f == score.frame && !score.isOver ? const Color(0xFFFFC840) : _box),
              ),
              child: Column(
                children: [
                  Text(score.marks(f).join(' '), style: const TextStyle(fontSize: 12, color: _ink)),
                  Text(
                    f < totals.length && totals[f] != null ? '${totals[f]}' : '',
                    style: const TextStyle(fontSize: 15, fontWeight: FontWeight.bold, color: _ink),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
