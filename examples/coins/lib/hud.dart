// What the game shows over the view, in plain Flutter.

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/widgets.dart';

/// The player's score, top left, kept up to date by a
/// [ValueListenableBuilder]; and, top right, the toon switch, when given.
class Hud extends StatelessWidget {
  const Hud(this.score, {this.toon, super.key});

  final ValueListenable<int> score;

  /// Whether the game is drawn in the toon style: a tap on the switch
  /// turns it on and off.
  final ValueNotifier<bool>? toon;

  static const _gold = Color(0xFFFFC840);

  @override
  Widget build(BuildContext context) => SafeArea(
    child: Padding(
      padding: const EdgeInsets.all(20),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          ValueListenableBuilder(
            valueListenable: score,
            builder: (context, score, _) => Text(
              '● $score',
              style: const TextStyle(fontSize: 32, fontWeight: FontWeight.bold, color: _gold),
            ),
          ),
          const Spacer(),
          if (toon case final toon?)
            GestureDetector(
              onTap: () => toon.value = !toon.value,
              child: ValueListenableBuilder(
                valueListenable: toon,
                builder: (context, on, _) => Container(
                  padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
                  decoration: BoxDecoration(
                    color: on ? _gold : const Color(0x66000000),
                    borderRadius: BorderRadius.circular(20),
                  ),
                  child: Text(
                    on ? 'Toon: on' : 'Toon: off',
                    style: TextStyle(
                      fontSize: 18,
                      fontWeight: FontWeight.bold,
                      color: on ? const Color(0xFF1C1626) : _gold,
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    ),
  );
}
