// Sheepdog for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart: the game with its HUD and minimap at level 3, with
// wolves, and nobody playing, so the wolves take some of the flock. Its
// lifecycle check starts a new round again and again. The tool's app is
// dark, so it's night in the field, the lamps on; `--define
// SHEEPDOG_QUALITY=low` (or medium, high) draws it at that level, the
// device's pick otherwise.

import 'package:flutter/material.dart' show ThemeData;
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
          if (!_probe.contains('nomap')) Minimap(game),
          Hud(game, onPause: () {}),
        ],
      );
    },
    change: (game) => (game as SheepdogGame).restart(),
  ),
);
