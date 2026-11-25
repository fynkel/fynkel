import 'package:flutter/material.dart';

import 'game.dart';

/// Everything Flutter draws over the game: the title, the score and lives
/// while playing, and the game over panel.
class Screens extends StatelessWidget {
  const Screens(this.game, {super.key});

  final StarfallGame game;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: game.phase,
    builder: (context, phase, _) => switch (phase) {
      Phase.title => _Panel(
        title: 'Starfall',
        lines: ['Catch the stars, not the rocks.', 'Best: ${game.best.value}'],
        button: 'Play',
        onPressed: game.start,
      ),
      Phase.playing => _Hud(game),
      Phase.over => _Panel(
        title: 'Game over',
        lines: ['Score: ${game.score.value}', 'Best: ${game.best.value}'],
        button: 'Play again',
        onPressed: game.start,
      ),
    },
  );
}

class _Hud extends StatelessWidget {
  const _Hud(this.game);

  final StarfallGame game;

  @override
  Widget build(BuildContext context) => SafeArea(
    child: Padding(
      padding: const EdgeInsets.all(16),
      child: ListenableBuilder(
        listenable: Listenable.merge([game.score, game.lives]),
        builder: (context, _) => Row(
          children: [
            Text('${game.score.value}', style: _big),
            const Spacer(),
            Text('♥' * game.lives.value, style: _big.copyWith(color: const Color(0xFFFF6A6A))),
          ],
        ),
      ),
    ),
  );
}

class _Panel extends StatelessWidget {
  const _Panel({required this.title, required this.lines, required this.button, required this.onPressed});

  final String title;
  final List<String> lines;
  final String button;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) => Center(
    child: Card(
      color: const Color(0xE0202A48),
      child: Padding(
        padding: const EdgeInsets.all(28),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(title, style: _big.copyWith(fontSize: 36)),
            const SizedBox(height: 12),
            for (final line in lines) Text(line, style: const TextStyle(color: Colors.white70, fontSize: 18)),
            const SizedBox(height: 20),
            FilledButton(onPressed: onPressed, child: Text(button)),
          ],
        ),
      ),
    ),
  );
}

const _big = TextStyle(color: Colors.white, fontSize: 28, fontWeight: FontWeight.bold);
