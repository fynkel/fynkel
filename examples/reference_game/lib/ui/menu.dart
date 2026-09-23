// The game's screens (M3-15): the title over the Menu scene, the Results
// over its scene, the pause menu over a frozen Playground. Plain Flutter
// widgets on the game view's overlay: the engine draws the backdrop, the
// buttons call `Game.play/finish/toMenu` (a scene transition at the next
// frame) or the page's own callbacks (pause, settings). The screens hold
// no game state — `GameState.screen` picks which one is up (lib/main.dart)
// and `GameState.lastRun` is the results.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../game/game.dart';
import '../game/game_state.dart';

/// Whether the platform lets an app quit itself (a phone does not).
bool get canQuit => Platform.isMacOS || Platform.isLinux || Platform.isWindows;

/// The title screen: Play, Settings, Quit (desktop).
class MenuScreen extends StatelessWidget {
  const MenuScreen({super.key, required this.game, required this.onSettings});

  final Game game;
  final VoidCallback onSettings;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context).textTheme;
    return MenuPanel(
      game: game,
      children: [
        Text('Fynkel Reference Game', style: theme.headlineLarge?.copyWith(fontWeight: FontWeight.bold)),
        const SizedBox(height: 4),
        Text('Ten coins. One sweeper. A gate.', style: theme.titleMedium),
        const SizedBox(height: 24),
        MenuButton(game: game, label: 'Play', primary: true, onPressed: game.play),
        MenuButton(game: game, label: 'Settings', onPressed: onSettings),
        if (canQuit) MenuButton(game: game, label: 'Quit', onPressed: FynkelApplication.quit),
      ],
    );
  }
}

/// The end of a run: the numbers, Play again, Menu.
class ResultsScreen extends StatelessWidget {
  const ResultsScreen({super.key, required this.game});

  final Game game;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context).textTheme;
    final state = ModelBinding.of<GameState>(context);
    return ModelBuilder<RunResult?>(
      value: state.lastRun,
      builder: (context, run, _) => MenuPanel(
        game: game,
        children: [
          Text(
            run?.won ?? false ? 'All coins collected!' : 'Run over',
            style: theme.headlineLarge?.copyWith(fontWeight: FontWeight.bold),
          ),
          const SizedBox(height: 16),
          if (run != null)
            DefaultTextStyle(
              style: theme.titleLarge!,
              child: Table(
                defaultColumnWidth: const IntrinsicColumnWidth(),
                children: [
                  for (final (label, value) in [
                    ('Coins', '${run.collected} / ${run.total}'),
                    ('Time', run.clock),
                    ('Deaths', '${run.deaths}'),
                    ('Steps', '${run.footsteps}'),
                  ])
                    TableRow(
                      children: [
                        Padding(padding: const EdgeInsets.only(right: 24), child: Text(label)),
                        Text(value, textAlign: TextAlign.right),
                      ],
                    ),
                ],
              ),
            ),
          const SizedBox(height: 24),
          MenuButton(game: game, label: 'Play again', primary: true, onPressed: game.play),
          MenuButton(game: game, label: 'Menu', onPressed: game.toMenu),
        ],
      ),
    );
  }
}

/// The pause menu over the frozen level: Resume, Settings, Give up (the
/// Results with what was collected), Quit to menu.
class PauseMenu extends StatelessWidget {
  const PauseMenu({super.key, required this.game, required this.onResume, required this.onSettings});

  final Game game;
  final VoidCallback onResume;
  final VoidCallback onSettings;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context).textTheme;
    return MenuPanel(
      game: game,
      children: [
        Text('Paused', style: theme.headlineLarge?.copyWith(fontWeight: FontWeight.bold)),
        const SizedBox(height: 24),
        MenuButton(game: game, label: 'Resume (Esc)', primary: true, onPressed: onResume),
        MenuButton(game: game, label: 'Settings', onPressed: onSettings),
        MenuButton(
          game: game,
          label: 'Give up',
          onPressed: () {
            onResume(); // the transition runs at the next frame: the loop must tick
            game.finish();
          },
        ),
        MenuButton(
          game: game,
          label: 'Quit to menu',
          onPressed: () {
            onResume();
            game.toMenu();
          },
        ),
      ],
    );
  }
}

/// A dimmed backdrop with a centred column; every screen is one.
class MenuPanel extends StatelessWidget {
  const MenuPanel({super.key, required this.game, required this.children});

  final Game game;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return ColoredBox(
      color: Colors.black.withValues(alpha: 0.45),
      child: Center(
        child: SingleChildScrollView(
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(mainAxisSize: MainAxisSize.min, children: children),
          ),
        ),
      ),
    );
  }
}

/// A menu button that clicks (assets/audio/click.ogg on the presentation
/// lane: heard at once, outside the ticks, never in a replay).
class MenuButton extends StatelessWidget {
  const MenuButton({super.key, required this.game, required this.label, required this.onPressed, this.primary = false});

  final Game game;
  final String label;
  final VoidCallback onPressed;
  final bool primary;

  @override
  Widget build(BuildContext context) {
    void press() {
      game.world.audio.play(clickSound, volume: 0.6, presentation: true);
      onPressed();
    }

    final child = SizedBox(width: 220, child: Text(label, textAlign: TextAlign.center));
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: primary ? FilledButton(onPressed: press, child: child) : OutlinedButton(onPressed: press, child: child),
    );
  }
}
