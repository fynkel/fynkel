// Stage 7: Flutter beside the game, the basics. The scene's widgets are
// built with its world, which is there from the start (FynkelGame adds the
// root before it builds): a stick Flutter draws feeds the 'move' axis, and
// a banner shows itself while the game is paused, from wherever it's
// paused (world.pausedListenable).

import 'package:flutter/material.dart' hide Color, Material, Transform;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

class FlutterBasicsScene extends LabScene {
  @override
  String get title => 'Flutter: beside the game';
  @override
  String get step => 'stage 7';
  @override
  String get about =>
      'Drag the stick at the bottom left: a Flutter widget, made with the scene\'s world.input, feeding the '
      "'move' axis that WASD feed too. P or the pause switch pauses the game; the banner over it shows "
      'itself from world.pausedListenable, and tapping it goes on (paused, the game hears no keys).';

  late final GameObject cube;

  @override
  void build() {
    cube = add(shape('cube', Mesh.cube(), const Color(0xFF4CA6E0), at: Vec3(0, 0.5, 0)));
    on(world.onPausedChanged, (paused) => log.add(paused ? 'paused' : 'going on'));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    // Paused, the game hears no keys: the banner or the switch goes on.
    if (input.wasPressed(KeyCode.p)) world.paused = true;
    final move = input.axis2('move');
    cube.position += Vec3(move.x, 0, -move.y) * (4 * time.delta);
    readout.value = 'move ${move.x.toStringAsFixed(2)}, ${move.y.toStringAsFixed(2)}';
  }

  @override
  List<Control> get controls => [Toggle('paused', () => world.paused, (on) => world.paused = on)];

  @override
  Widget overlay(BuildContext context) => Stack(
    children: [
      Positioned(left: 24, bottom: 24, child: Stick(world.input)),
      ValueListenableBuilder(
        valueListenable: world.pausedListenable,
        builder: (context, paused, _) => paused
            ? Center(
                child: FilledButton(onPressed: () => world.paused = false, child: const Text('Paused · tap to go on')),
              )
            : const SizedBox.shrink(),
      ),
    ],
  );
}
