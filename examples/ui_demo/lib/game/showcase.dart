// The UI demo's extras (ADR-0062 amendment 1): a crowd with a health bar
// over every member (`WorldMarks`, one painter), a minimap and a camera
// that follows the cube (`CameraView`s), all made from code at the start
// of each play — a scene load (Main menu → Play) replaces runtime objects,
// so [ensure] builds them again when they are gone.

import 'dart:math' as math;
import 'dart:ui' show Color, Size;

import 'package:fynkel/gameplay.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

class Showcase {
  Showcase(this.world, {this.crowdSize = 300})
    : bars = WorldMarks(
        world,
        style: const MarkStyle.bar(
          size: Size(28, 4),
          color: Color(0xFF66BB6A),
          background: Color(0xAA000000),
          maxDistance: 30,
        ),
        offset: const Vec3(0, 0.35, 0),
      );

  final World world;
  final int crowdSize;

  /// One bar per crowd member.
  final WorldMarks bars;

  final List<GameObject> crowd = [];

  /// An orthographic camera high over the cube, looking down.
  GameObject? minimap;

  /// A perspective camera that follows the cube from behind.
  GameObject? cubeCam;

  /// Builds whatever a scene load took away; true when something was built
  /// (the widgets showing the cameras rebuild then).
  bool ensure() {
    final cube = world.find('Cube');
    if (cube == null) return false;
    final crowdGone = crowd.isEmpty || !crowd.first.isAlive;
    final camsGone = minimap == null || !minimap!.isAlive;
    if (!crowdGone && !camsGone) return false;
    world.edit(() {
      if (crowdGone) _buildCrowd();
      if (camsGone) _buildCameras(cube);
    });
    return true;
  }

  void _buildCrowd() {
    bars.clear();
    crowd.clear();
    final columns = math.sqrt(crowdSize * 1.5).ceil();
    for (var i = 0; i < crowdSize; i++) {
      final x = -9 + 18 * (i % columns) / math.max(columns - 1, 1);
      final z = -9 + 7.5 * (i ~/ columns) / math.max(crowdSize ~/ columns, 1);
      final o = world.build(
        ObjectSpec.primitive(
          Primitive.cube,
          name: 'Crowd $i',
          transform: Pose(position: Vec3(x, 0.15, z), scale: const Vec3(0.3, 0.3, 0.3)),
          components: [Oscillate(amplitude: 0.12, frequency: 0.3 + (i % 7) * 0.08, phase: i * 0.13)],
        ),
      );
      crowd.add(o);
      bars.add(o);
    }
  }

  void _buildCameras(GameObject cube) {
    final down = world.spawn(
      name: 'Minimap camera',
      transform: Pose(position: const Vec3(0, 20, 0), rotation: Quat.axisAngle(Vec3.right, -math.pi / 2)),
    );
    // Below the main camera's priority (0): the game keeps its own view.
    down.set(const Camera(projection: Projection.orthographic, orthoHeight: 22, priority: -1));
    minimap = down;

    final follow = world.spawn(name: 'Cube camera', transform: const Pose(position: Vec3(0, 2, 3)));
    follow.set(const Camera(fovY: 0.9, priority: -2));
    follow.set(Follow(target: cube, offset: const Vec3(0, 1.4, 2.6), smoothing: 4, lookAt: true));
    cubeCam = follow;
  }

  /// Health that comes and goes, so the bars visibly change: red when low.
  void tick(double seconds) {
    final marks = bars.marks;
    for (var i = 0; i < marks.length; i++) {
      final v = 0.5 + 0.5 * math.sin(seconds * 0.8 + i * 0.37);
      marks[i]
        ..value = v
        ..color = v < 0.3 ? const Color(0xFFE53935) : null;
    }
  }

  void dispose() => bars.dispose();
}
