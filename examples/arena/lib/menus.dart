// The app around the game, in plain Flutter: a title screen, the game's
// page, its pause menu, and a settings page. A page or dialog over the
// game pauses it by itself (FynkelView's pauseWhenCovered), and closing
// the last one goes on: no pause code here.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'arena.dart';
import 'hud.dart';

/// What the player chose in the settings page.
class Settings {
  final themeMode = ValueNotifier(ThemeMode.dark);
}

/// The whole app: the title screen first.
class ArenaApp extends StatelessWidget {
  ArenaApp({super.key, Settings? settings, this.seed}) : settings = settings ?? Settings();

  final Settings settings;

  /// Makes every round the same, for tests.
  final int? seed;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: settings.themeMode,
    builder: (context, mode, _) => MaterialApp(
      title: 'Arena',
      theme: ThemeData(colorSchemeSeed: const Color(0xFFB8F27C)),
      darkTheme: ThemeData(colorSchemeSeed: const Color(0xFFB8F27C), brightness: Brightness.dark),
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
  Widget build(BuildContext context) => Scaffold(
    body: Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text('Arena', style: Theme.of(context).textTheme.displayMedium),
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

/// The game, with its HUD. Back (Android's button, Escape, P, or the
/// HUD's pause button) opens the pause menu.
class PlayPage extends StatelessWidget {
  const PlayPage({super.key, required this.settings, this.seed});

  final Settings settings;
  final int? seed;

  @override
  Widget build(BuildContext context) => FynkelGame(
    make: () => ArenaGame(seed: seed),
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
          child: Stack(
            children: [
              FynkelView(root: game),
              Hud(game, onPause: pause),
            ],
          ),
        ),
      );
    },
  );
}

/// The pause menu, over the game's page at [context]. Closing it (Resume,
/// Escape or P, a tap beside it) goes on; back leaves the game.
Future<void> showPauseMenu(BuildContext context, ArenaGame game, Settings settings) {
  final navigator = Navigator.of(context);
  void leave() {
    navigator.pop(); // The menu.
    navigator.pop(); // The game.
  }

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
              SimpleDialogOption(onPressed: leave, child: const Text('Leave')),
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
      ],
    ),
  );
}
