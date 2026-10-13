// Stage 7: a minimap. A second FynkelView of the same world, in the corner
// over the game, drawn from a camera of its own that looks straight down
// at the player and follows it. The world is updated once a frame for
// both views, and its meshes are on the GPU once. The minimap takes no
// input: a tap on it reaches neither the game nor the view under it.

import 'dart:math' as math;

import 'package:flutter/material.dart' hide Color, Material, Transform;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

class MinimapBasicsScene extends LabScene {
  @override
  String get title => 'Minimap: the basics';
  @override
  String get step => 'stage 7';
  @override
  String get about =>
      'WASD or the arrows move the cube. The square at the top right is a second view of the same world, '
      'from a camera of its own above the cube; it follows the cube, and the pillars show where it is. '
      'The readout has each view\'s frames per second.';

  late final GameObject player;
  late final GameObject above;
  late final Camera map;
  var _game = '–', _map = '–';

  @override
  void build() {
    player = add(shape('player', Mesh.cube(), const Color(0xFF4CA6E0), at: Vec3(0, 0.5, 0)));
    const colours = [0xFFE0613A, 0xFFFFD040, 0xFF6CC070, 0xFFB070E0, 0xFFE070A0, 0xFF60D0D0];
    for (var i = 0; i < colours.length; i++) {
      final angle = i * 2 * math.pi / colours.length;
      add(
        shape(
          'pillar $i',
          Mesh.cube(),
          Color(colours[i]),
          at: Vec3(7 * math.cos(angle), 1, 7 * math.sin(angle)),
          size: Vec3(0.6, 2, 0.6),
        ),
      );
    }
    // Looking straight down, with the world's -Z (away from the main
    // camera) at the top of the map.
    map = Camera(orthographic: true, orthographicSize: 6, clearColor: const Color(0xFF101318));
    above = add(
      GameObject(
        name: 'minimap camera',
        transform: Transform(
          position: Vec3(0, 20, 0),
          rotation: Quat.lookRotation(Vec3(0, -1, 0), up: Vec3(0, 0, -1)),
        ),
        components: [map],
      ),
    );
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final move = input.axis2('move');
    final p = player.position + Vec3(move.x, 0, -move.y) * (5 * time.delta);
    player.position = Vec3(p.x.clamp(-9, 9), 0.5, p.z.clamp(-9, 9));
    above.position = Vec3(player.position.x, 20, player.position.z);
    readout.value = 'game $_game fps   minimap $_map fps';
  }

  @override
  List<Control> get controls => const [];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _game = fps('game', s));

  @override
  Widget overlay(BuildContext context) => Align(
    alignment: Alignment.topRight,
    child: Padding(
      padding: const EdgeInsets.all(12),
      child: Container(
        width: 160,
        height: 160,
        decoration: BoxDecoration(border: Border.all(color: Colors.white70, width: 2)),
        child: FynkelView(
          root: this,
          camera: map,
          input: false,
          worldWidgets: false,
          onStats: (s) => _map = fps('minimap', s),
        ),
      ),
    ),
  );
}
