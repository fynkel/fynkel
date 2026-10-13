// What the game shows over the view, in plain Flutter: the score and the
// wave, the hearts, a pause button, and what to do when paused or over;
// and the health bars over the enemies, in the world.

import 'package:flutter/widgets.dart';

import 'arena.dart';

class Hud extends StatelessWidget {
  const Hud(this.game, {super.key});

  final ArenaGame game;

  static const _text = TextStyle(fontSize: 24, fontWeight: FontWeight.bold, color: Color(0xFFFFFFFF));

  @override
  Widget build(BuildContext context) => SafeArea(
    child: Padding(
      padding: const EdgeInsets.all(20),
      child: Stack(
        children: [
          Align(
            alignment: Alignment.topLeft,
            child: ListenableBuilder(
              listenable: Listenable.merge([game.score, game.wave]),
              builder: (context, _) => Text('wave ${game.wave.value}   ★ ${game.score.value}', style: _text),
            ),
          ),
          Align(
            alignment: Alignment.topRight,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                ValueListenableBuilder(
                  valueListenable: game.health,
                  builder: (context, left, _) => Text(
                    '♥' * left + '♡' * (Player.hearts - left),
                    style: _text.copyWith(color: const Color(0xFFFF6FA0)),
                  ),
                ),
                const SizedBox(width: 20),
                _Button(listenable: game.paused, label: () => game.paused.value ? '▶' : 'II', onTap: game.togglePause),
              ],
            ),
          ),
          Center(
            child: ListenableBuilder(
              listenable: Listenable.merge([game.paused, game.over]),
              builder: (context, _) => switch ((game.paused.value, game.over.value)) {
                (_, true) => Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text('game over', style: _text.copyWith(fontSize: 40)),
                    const SizedBox(height: 16),
                    _Button(listenable: game.over, label: () => 'again', onTap: game.restart),
                  ],
                ),
                (true, _) => Text('paused', style: _text.copyWith(fontSize: 40)),
                _ => const SizedBox.shrink(),
              },
            ),
          ),
        ],
      ),
    ),
  );
}

class _Button extends StatelessWidget {
  const _Button({required this.listenable, required this.label, required this.onTap});

  final Listenable listenable;
  final String Function() label;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) => GestureDetector(
    onTap: onTap,
    child: Container(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      decoration: BoxDecoration(color: const Color(0x66000000), borderRadius: BorderRadius.circular(8)),
      child: ListenableBuilder(
        listenable: listenable,
        builder: (context, _) => Text(label(), style: Hud._text),
      ),
    ),
  );
}

/// An enemy's health, over it in the world (its WorldWidget): a bar that
/// empties as it's hit.
class HealthBar extends StatelessWidget {
  const HealthBar(this.health, {super.key});

  final Health health;

  @override
  Widget build(BuildContext context) => StreamBuilder(
    stream: health.hurt.stream,
    initialData: health.left,
    builder: (context, left) => Container(
      width: 40,
      height: 7,
      padding: const EdgeInsets.all(1),
      decoration: BoxDecoration(color: const Color(0xAA000000), borderRadius: BorderRadius.circular(3)),
      alignment: Alignment.centerLeft,
      child: FractionallySizedBox(
        widthFactor: left.requireData / health.max,
        heightFactor: 1,
        child: DecoratedBox(
          decoration: BoxDecoration(color: const Color(0xFFB8F27C), borderRadius: BorderRadius.circular(2)),
        ),
      ),
    ),
  );
}
