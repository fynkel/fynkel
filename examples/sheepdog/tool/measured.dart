// Sheepdog for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart: the game with its HUD and minimap at level 3, with
// wolves, and nobody playing, so the wolves take some of the flock. Its
// lifecycle check starts a new round again and again. The tool's app is
// dark, so it's night in the field, the lamps on; `--define
// SHEEPDOG_QUALITY=low` (or medium, high) draws it at that level, the
// device's pick otherwise.

import 'package:flutter/material.dart' show AlertDialog, ThemeData, showDialog;
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show Light, LightKind, Quality;
import 'package:fynkel_measure/fynkel_measure.dart';
import 'package:sheepdog/game.dart';
import 'package:sheepdog/hud.dart';

const _quality = String.fromEnvironment('SHEEPDOG_QUALITY');
const _probe = String.fromEnvironment('SHEEPDOG_PROBE');

SheepdogGame _make() {
  final game = SheepdogGame(
    level: 3,
    quality: ValueNotifier(Quality.levels.where((q) => q.name == _quality).firstOrNull),
  );
  game.loaded.then((_) {
    final camera = game.camera.camera;
    if (_probe.contains('noao')) camera.ambientOcclusion = null;
    if (_probe.contains('nobloom')) camera.bloom = null;
    if (_probe.contains('day')) game.wear(ThemeData.light());
    if (_probe.contains('noshadows')) {
      for (final l in game.world.all<Light>()) {
        l.shadows = false;
      }
    }
    // Paused, as behind the pause menu: what a still scene costs
    // (docs/iphone-heat.md).
    if (_probe.contains('paused')) game.world.paused = true;
    if (_probe.contains('nolamps')) {
      for (final l in game.world.all<Light>().where((l) => l.kind == LightKind.point)) {
        l.enabled = false;
      }
    }
  });
  return game;
}

void main() => runApp(
  MeasuredApp(
    _make,
    overlay: (game) {
      game as SheepdogGame;
      return Stack(
        fit: StackFit.expand,
        children: [
          // A menu over the game, as the pause menu: the view pauses it.
          if (_probe.contains('menu')) const _Menu(),
          if (!_probe.contains('nomap')) Minimap(game),
          Hud(game, onPause: () {}),
        ],
      );
    },
    change: (game) => (game as SheepdogGame).restart(),
  ),
);

/// Opens a dialog over the game once it's built, and leaves it open.
class _Menu extends StatefulWidget {
  const _Menu();

  @override
  State<_Menu> createState() => _MenuState();
}

class _MenuState extends State<_Menu> {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      showDialog<void>(
        context: context,
        builder: (_) => const AlertDialog(title: Text('Paused')),
      );
    });
  }

  @override
  Widget build(BuildContext context) => const SizedBox.shrink();
}
