import 'dart:math' as math;

import 'package:fynkel/gameplay.dart';

import 'pickup.dart';
import 'pickup_kind.dart';

/// The pickup as code (ADR-0026, a code prefab): a gem spinning above the
/// ground and bobbing, a `pickup` sensor, and the [Pickup] of [kind] —
/// what tool/assets.dart writes as assets/prefabs/pickup.prefab.json.
/// The gem is the built-in cube turned on its corner, in [material].
ObjectSpec pickup(
  PickupKind? kind,
  AssetRef material, {
  String name = 'Pickup',
  Pose transform = const Pose(),
}) => ObjectSpec(
  name,
  transform: transform,
  components: const [
    Rotator(speed: 1.8, space: MotionSpace.world),
    Collider(shape: ColliderShape.sphere, size: Vec3(0.9, 0.9, 0.9), isSensor: true),
    TriggerZone(event: 'pickup', once: true),
  ],
  behaviours: [Pickup()..kind = kind],
  children: [
    ObjectSpec(
      'Gem',
      transform: Pose(
        position: const Vec3(0, 0.8, 0),
        rotation: Quat.axisAngle(const Vec3(0, 0, 1), math.pi / 4) * Quat.axisAngle(Vec3.right, math.pi / 4),
        scale: const Vec3(0.45, 0.45, 0.45),
      ),
      components: [
        MeshRenderer(mesh: Primitive.cube.mesh, material: material),
        const Oscillate(axis: Vec3(0, 1, 0), amplitude: 0.15, frequency: 0.6),
      ],
    ),
  ],
);
