// What the game shows over the view, in plain Flutter: the score and the
// wave, the hearts, a pause button, and what to do when it's over; and the
// health bars over the enemies, in the world.

import 'package:flutter/widgets.dart';

import 'arena.dart';

class Hud extends StatelessWidget {
  /// The HUD of [game]; its pause button calls [onPause], if given.
  const Hud(this.game, {super.key, this.onPause});

  final ArenaGame game;
  final VoidCallback? onPause;

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
                if (onPause case final onPause?) ...[const SizedBox(width: 20), _Button(label: 'II', onTap: onPause)],
              ],
            ),
          ),
          Center(
            child: ValueListenableBuilder(
              valueListenable: game.over,
              builder: (context, over, _) => over
                  ? Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text('game over', style: _text.copyWith(fontSize: 40)),
                        const SizedBox(height: 16),
                        _Button(label: 'again', onTap: game.restart),
                      ],
                    )
                  : const SizedBox.shrink(),
            ),
          ),
        ],
      ),
    ),
  );
}

class _Button extends StatelessWidget {
  const _Button({required this.label, required this.onTap});

  final String label;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) => GestureDetector(
    onTap: onTap,
    child: Container(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      decoration: BoxDecoration(color: const Color(0x66000000), borderRadius: BorderRadius.circular(8)),
      child: Text(label, style: Hud._text),
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
