// What the game shows over the view, in plain Flutter.

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/widgets.dart';

/// The player's score, top left, kept up to date by a
/// [ValueListenableBuilder].
class Hud extends StatelessWidget {
  const Hud(this.score, {super.key});

  final ValueListenable<int> score;

  @override
  Widget build(BuildContext context) => Align(
    alignment: Alignment.topLeft,
    child: SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: ValueListenableBuilder(
          valueListenable: score,
          builder: (context, score, _) => Text(
            '● $score',
            style: const TextStyle(fontSize: 32, fontWeight: FontWeight.bold, color: Color(0xFFFFC840)),
          ),
        ),
      ),
    ),
  );
}
