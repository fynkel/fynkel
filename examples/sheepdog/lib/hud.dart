// What the game shows over the view, in plain Flutter and the app's theme:
// the flock's count and the wave, a pause button, a bark button for touch,
// and the minimap, a second view of the same world.

import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';

class Hud extends StatelessWidget {
  /// The HUD of [game]; its pause button calls [onPause].
  const Hud(this.game, {super.key, required this.onPause});

  final SheepdogGame game;
  final VoidCallback onPause;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Stack(
          children: [
            Align(
              alignment: Alignment.topLeft,
              child: _Panel(
                child: ListenableBuilder(
                  listenable: Listenable.merge([game.home, game.lost, game.wave]),
                  builder: (context, _) => Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text('home ${game.home.value} of ${names.length}', style: text.titleLarge),
                      Text(
                        'lost ${game.lost.value}   ${game.wave.value == 0 ? 'no wolves yet' : 'wave ${game.wave.value}'}',
                        style: text.bodyMedium,
                      ),
                    ],
                  ),
                ),
              ),
            ),
            Align(
              alignment: Alignment.topRight,
              child: IconButton.filledTonal(onPressed: onPause, tooltip: 'Pause', icon: const Icon(Icons.pause)),
            ),
            Align(
              alignment: Alignment.bottomRight,
              child: FloatingActionButton.extended(
                heroTag: null,
                onPressed: () => game.round.dog.bark(),
                icon: const Icon(Icons.campaign),
                label: const Text('Bark'),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Panel extends StatelessWidget {
  const _Panel({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) => Container(
    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
    decoration: BoxDecoration(
      color: Theme.of(context).colorScheme.surface.withValues(alpha: 0.75),
      borderRadius: BorderRadius.circular(12),
    ),
    child: child,
  );
}

/// The whole field from above, in a corner: a second view of [game]'s
/// world, from its minimap camera. It takes no input, and shows no labels.
class Minimap extends StatelessWidget {
  const Minimap(this.game, {super.key});

  final SheepdogGame game;

  @override
  Widget build(BuildContext context) => SafeArea(
    child: Align(
      alignment: Alignment.topRight,
      child: Padding(
        padding: const EdgeInsets.only(top: 72, right: 16),
        child: Container(
          width: 140,
          height: 140,
          decoration: BoxDecoration(
            border: Border.all(color: Theme.of(context).colorScheme.outline, width: 2),
            borderRadius: BorderRadius.circular(4),
          ),
          child: FynkelView(root: game, camera: game.map, input: false, worldWidgets: false),
        ),
      ),
    ),
  );
}
