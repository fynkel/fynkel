// Sheepdog for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart: the game with its HUD and minimap at level 3, with
// wolves, and nobody playing, so the wolves take some of the flock. Its
// lifecycle check starts a new round again and again. The tool's app is
// dark, so it's night in the field, the lamps on; `--define
// SHEEPDOG_QUALITY=low` (or medium, high) draws it at that level, the
// device's pick otherwise; `--define SHEEPDOG_SCALE=0.75` at that render
// scale.

import 'package:flutter/material.dart' show AlertDialog, ThemeData, showDialog;
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show Blending, Color, GameObject, Light, LightKind, MeshRenderer, Quality;
import 'package:fynkel_measure/fynkel_measure.dart';
import 'package:sheepdog/game.dart';
import 'package:sheepdog/hud.dart';

const _quality = String.fromEnvironment('SHEEPDOG_QUALITY');
const _probe = String.fromEnvironment('SHEEPDOG_PROBE');
const _scale = String.fromEnvironment('SHEEPDOG_SCALE');

SheepdogGame _make() {
  final game = SheepdogGame(
    level: 3,
    quality: ValueNotifier(Quality.levels.where((q) => q.name == _quality).firstOrNull),
  );
  game.loaded.then((_) {
    // Its level drawn at a render scale of its own (set on the world, so
    // the game keeps what it picked for the level).
    if (_scale.isNotEmpty) {
      game.world.quality = (game.world.quality ?? Quality.device).copyWith(renderScale: double.parse(_scale));
    }
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
    onStats: _shaderProbes,
  ),
);

/// The shaders' probes: `nograss` and `nopond` hide them, `noground`
/// draws the field with the engine's own shader. Again each second, as a
/// new round makes them again.
void _shaderProbes(GameObject root, Object _) {
  final game = root as SheepdogGame;
  if (_probe.contains('noground')) game.grass.shader = null;
  if (_probe.contains('plainpond') && game.water.shader != null) {
    game.water
      ..shader = null
      ..color = const Color(0xA00C4868)
      ..blending = Blending.transparent;
  }
  for (final r in game.world.all<MeshRenderer>()) {
    if ((_probe.contains('nograss') && r.material == game.blades) ||
        (_probe.contains('nopond') && r.material == game.water)) {
      r.enabled = false;
    }
  }
}

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
