import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

/// The scenery: a dark hill under the basket, a moon, and far stars. None
/// of it moves, so it costs next to nothing each frame.
GameObject night() {
  final random = math.Random(4);
  final far = Material(color: const Color(0xFFFFFFFF), emissive: const Color(0xFFCFD8FF));
  return GameObject(
    name: 'night',
    children: [
      GameObject(
        name: 'ground',
        transform: Transform(position: Vec3(0, -2.5, -15), scale: Vec3(90, 2, 50)),
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF1C2B3A), roughness: 1))],
      ),
      GameObject(
        name: 'moon',
        transform: Transform(position: Vec3(9, 22, -40), scale: Vec3.one * 5),
        components: [
          MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFFFFF4D6), emissive: const Color(0xFFE8DDBF))),
        ],
      ),
      for (var i = 0; i < 90; i++)
        GameObject(
          transform: Transform(
            position: Vec3(random.nextDouble() * 34 - 17, random.nextDouble() * 34 + 1, -35),
            scale: Vec3.one * (0.08 + random.nextDouble() * 0.12),
          ),
          components: [MeshRenderer(Mesh.sphere(), far)],
        ),
    ],
  );
}
