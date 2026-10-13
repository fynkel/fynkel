// The app around the game, in plain Flutter: a title screen, the game's
// page with its HUD and minimap, a pause menu, a settings page, and the
// pop-up at the end of a round. A page or dialog over the game pauses it
// by itself (FynkelView's pauseWhenCovered), and closing the last one goes
// on: there is no pause code here.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart' show KeyCode;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';
import 'hud.dart';
import 'palette.dart';

/// What the player chose in the settings page.
class Settings {
  final themeMode = ValueNotifier(ThemeMode.system);
  final minimap = ValueNotifier(true);
  final controls = Controls();
}

/// The whole app: the title screen first.
class SheepdogApp extends StatelessWidget {
  SheepdogApp({super.key, Settings? settings, this.seed}) : settings = settings ?? Settings();

  final Settings settings;

  /// Makes every round the same, for tests.
  final int? seed;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: settings.themeMode,
    builder: (context, mode, _) => MaterialApp(
      title: 'Sheepdog',
      theme: sheepdogTheme(Brightness.light),
      darkTheme: sheepdogTheme(Brightness.dark),
      themeMode: mode,
      home: TitleScreen(settings: settings, seed: seed),
    ),
  );
}

class TitleScreen extends StatelessWidget {
  const TitleScreen({super.key, required this.settings, this.seed});

  final Settings settings;
  final int? seed;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return Scaffold(
      body: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text('Sheepdog', style: text.displayMedium),
            const SizedBox(height: 8),
            Text('Bring the flock home before the time is up.', style: text.bodyLarge),
            const SizedBox(height: 32),
            FilledButton(
              autofocus: true,
              onPressed: () => Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (_) => PlayPage(settings: settings, seed: seed),
                ),
              ),
              child: const Text('Play'),
            ),
            const SizedBox(height: 12),
            OutlinedButton(onPressed: () => openSettings(context, settings), child: const Text('Settings')),
          ],
        ),
      ),
    );
  }
}

/// The game, with its HUD and minimap. Back (Android's button, Escape, P,
/// or the HUD's pause button) opens the pause menu; the end of a round
/// opens its pop-up.
class PlayPage extends StatelessWidget {
  const PlayPage({super.key, required this.settings, this.seed});

  final Settings settings;
  final int? seed;

  @override
  Widget build(BuildContext context) => FynkelGame(
    make: () => SheepdogGame(seed: seed, controls: settings.controls),
    builder: (context, game) {
      void pause() => showPauseMenu(context, game, settings);
      return PopScope(
        canPop: false,
        onPopInvokedWithResult: (didPop, _) {
          if (!didPop) pause();
        },
        child: CallbackShortcuts(
          bindings: {
            const SingleActivator(LogicalKeyboardKey.escape): pause,
            const SingleActivator(LogicalKeyboardKey.keyP): pause,
          },
          child: _RoundEnd(
            game,
            child: Material(
              type: MaterialType.transparency,
              child: Stack(
                fit: StackFit.expand,
                children: [
                  FynkelView(root: game),
                  ValueListenableBuilder(
                    valueListenable: settings.minimap,
                    builder: (context, shown, _) => shown ? Minimap(game) : const SizedBox.shrink(),
                  ),
                  Hud(game, onPause: pause),
                ],
              ),
            ),
          ),
        ),
      );
    },
  );
}

/// Shows the round's end pop-up when [game] says it's over.
class _RoundEnd extends StatefulWidget {
  const _RoundEnd(this.game, {required this.child});

  final SheepdogGame game;
  final Widget child;

  @override
  State<_RoundEnd> createState() => _RoundEndState();
}

class _RoundEndState extends State<_RoundEnd> {
  @override
  void initState() {
    super.initState();
    widget.game.result.addListener(_ended);
  }

  @override
  void dispose() {
    widget.game.result.removeListener(_ended);
    super.dispose();
  }

  void _ended() {
    final home = widget.game.result.value;
    if (home != null) showRoundEnd(context, widget.game, home);
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

/// The pop-up at the end of a round, over the game's page at [context]:
/// how many sheep came [home], its stars, and what next: the same level
/// again, or the next one for a star or more.
Future<void> showRoundEnd(BuildContext context, SheepdogGame game, int home) {
  final navigator = Navigator.of(context);
  final all = home == names.length;
  final stars = starsFor(home), lost = game.lost.value, out = names.length - home - lost;
  final scheme = Theme.of(context).colorScheme;
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (context) => PopScope(
      canPop: false,
      child: AlertDialog(
        title: Text(
          all
              ? 'All home!'
              : home == 0
              ? 'None home'
              : '$home of ${names.length} home',
        ),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Semantics(
              label: '$stars of 3 stars',
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  for (var i = 0; i < 3; i++)
                    Icon(i < stars ? Icons.star : Icons.star_border, size: 40, color: scheme.primary),
                ],
              ),
            ),
            const SizedBox(height: 12),
            Text(
              [
                if (all) 'Not one sheep lost. Good dog.',
                if (lost > 0) 'The wolves took $lost.',
                if (out > 0) '$out still out in the field.',
              ].join(' '),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => navigator
              ..pop() // The pop-up.
              ..pop(), // The game.
            child: const Text('Quit to title'),
          ),
          TextButton(
            autofocus: stars == 0,
            onPressed: () {
              navigator.pop();
              game.restart();
            },
            child: const Text('Play again'),
          ),
          if (stars > 0)
            FilledButton(
              autofocus: true,
              onPressed: () {
                navigator.pop();
                game.restart(level: game.level.value + 1);
              },
              child: Text('Level ${game.level.value + 1}'),
            ),
        ],
      ),
    ),
  );
}

/// The pause menu, over the game's page at [context]. Closing it (Resume,
/// Escape or P, a tap beside it) goes on; back leaves the game.
Future<void> showPauseMenu(BuildContext context, SheepdogGame game, Settings settings) {
  final navigator = Navigator.of(context);
  void leave() => navigator
    ..pop() // The menu.
    ..pop(); // The game.

  return showDialog<void>(
    context: context,
    builder: (context) => PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) leave();
      },
      // Escape would be back, as on a page: here it closes the menu.
      child: CallbackShortcuts(
        bindings: {
          const SingleActivator(LogicalKeyboardKey.escape): navigator.pop,
          const SingleActivator(LogicalKeyboardKey.keyP): navigator.pop,
        },
        child: Focus(
          autofocus: true,
          child: SimpleDialog(
            title: const Text('Paused'),
            children: [
              SimpleDialogOption(onPressed: navigator.pop, child: const Text('Resume')),
              SimpleDialogOption(
                onPressed: () {
                  navigator.pop();
                  game.restart();
                },
                child: const Text('Start over'),
              ),
              SimpleDialogOption(onPressed: () => openSettings(context, settings), child: const Text('Settings')),
              SimpleDialogOption(onPressed: leave, child: const Text('Quit to title')),
            ],
          ),
        ),
      ),
    ),
  );
}

/// The settings page, over whatever is at [context].
Future<void> openSettings(BuildContext context, Settings settings) =>
    Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => SettingsPage(settings)));

class SettingsPage extends StatelessWidget {
  const SettingsPage(this.settings, {super.key});

  final Settings settings;

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('Settings')),
    body: ListView(
      children: [
        ListTile(
          title: const Text('Look'),
          subtitle: const Text('Dark is night in the field.'),
          trailing: ValueListenableBuilder(
            valueListenable: settings.themeMode,
            builder: (context, mode, _) => SegmentedButton(
              segments: const [
                ButtonSegment(value: ThemeMode.light, label: Text('Light')),
                ButtonSegment(value: ThemeMode.dark, label: Text('Dark')),
                ButtonSegment(value: ThemeMode.system, label: Text('System')),
              ],
              selected: {mode},
              onSelectionChanged: (chosen) => settings.themeMode.value = chosen.single,
            ),
          ),
        ),
        ValueListenableBuilder(
          valueListenable: settings.minimap,
          builder: (context, shown, _) => SwitchListTile(
            title: const Text('Minimap'),
            value: shown,
            onChanged: (on) => settings.minimap.value = on,
          ),
        ),
        const Divider(),
        ListTile(
          title: Text('Controls', style: Theme.of(context).textTheme.titleMedium),
          subtitle: const Text('The arrow keys always move too.'),
          trailing: TextButton(onPressed: settings.controls.reset, child: const Text('Reset')),
        ),
        ListenableBuilder(
          listenable: settings.controls,
          builder: (context, _) => Column(
            children: [
              for (final (name, what) in const [
                ('up', 'Up'),
                ('down', 'Down'),
                ('left', 'Left'),
                ('right', 'Right'),
                ('bark', 'Bark'),
              ])
                ListTile(
                  title: Text(what),
                  trailing: OutlinedButton(
                    onPressed: () async {
                      final key = await askForKey(context, what);
                      if (key != null) settings.controls.set(name, key);
                    },
                    child: Text(settings.controls[name].label),
                  ),
                ),
            ],
          ),
        ),
      ],
    ),
  );
}

/// Waits for the player to press a key for [what], and gives it; null
/// for Escape, or a key the game has none for.
Future<KeyCode?> askForKey(BuildContext context, String what) => showDialog<KeyCode>(
  context: context,
  builder: (context) => Focus(
    autofocus: true,
    onKeyEvent: (node, event) {
      if (event is KeyDownEvent) {
        Navigator.of(context).pop(event.logicalKey == LogicalKeyboardKey.escape ? null : keyCodeOf(event));
      }
      return KeyEventResult.handled;
    },
    child: AlertDialog(title: Text('Press a key for $what'), content: const Text('Escape keeps the one it has.')),
  ),
);
