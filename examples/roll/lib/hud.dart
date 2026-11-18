import 'package:flutter/material.dart';

import 'game.dart';

/// Gems left, top left; a message once they're all taken.
class Hud extends StatelessWidget {
  const Hud(this.game, {super.key});

  final RollGame game;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: game.gemsLeft,
    builder: (context, left, _) => Padding(
      padding: const EdgeInsets.all(24),
      child: Text(
        left > 0 ? 'Gems left: $left' : 'You found them all!',
        style: const TextStyle(fontSize: 28, color: Colors.white, fontWeight: FontWeight.bold),
      ),
    ),
  );
}
