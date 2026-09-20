// Physics value types (M3-08, blueprint §9): what a game reads back from
// the Rapier-backed simulation — a body's motion state, a query hit, the
// project's physics settings. Bodies and colliders themselves are the
// native components `RigidBody`/`Collider` (lib/generated/schema.g.dart),
// attached through `GameObject.add`/`set`; forces and queries go through
// `GameObject` and `World`.

import '../generated/schema.g.dart' as gen;
import '../math.dart';

export '../generated/schema.g.dart' show BodyOp, BodyType, Collider, ColliderShape, PhysicsQueryKind, RigidBody;

/// The project's `physics` settings as the world applied them on load
/// (`LoadReport.physics`).
class PhysicsSettings {
  const PhysicsSettings({this.gravity = const Vec3(0, -9.81, 0), this.layers = const ['default'], this.meshColliders = 0});

  factory PhysicsSettings.fromJson(Map<String, Object?> j) {
    final g = (j['gravity'] as List?)?.cast<num>();
    return PhysicsSettings(
      gravity: g == null || g.length != 3 ? const Vec3(0, -9.81, 0) : Vec3(g[0].toDouble(), g[1].toDouble(), g[2].toDouble()),
      layers: [for (final l in (j['layers'] as List?) ?? const ['default']) l as String],
      meshColliders: (j['meshColliders'] as num?)?.toInt() ?? 0,
    );
  }

  /// Metres per second squared.
  final Vec3 gravity;

  /// Layer names, bit `i` of a collider's `layer`/`mask` is `layers[i]`.
  final List<String> layers;

  /// Mesh colliders registered from the scene's cooked meshes.
  final int meshColliders;

  /// The bit mask of the named layers, or null when a name is unknown.
  int? mask(Iterable<String> names) {
    var bits = 0;
    for (final n in names) {
      final i = layers.indexOf(n);
      if (i < 0) return null;
      bits |= 1 << i;
    }
    return bits;
  }
}

/// A rigid body's motion as the last step left it (`GameObject.bodyState`).
class BodyState {
  const BodyState({required this.linearVelocity, required this.angularVelocity, required this.sleeping});

  factory BodyState.fromRecord(gen.BodyStateRecord r) =>
      BodyState(linearVelocity: r.linearVelocity, angularVelocity: r.angularVelocity, sleeping: r.sleeping);

  /// Metres per second, world space.
  final Vec3 linearVelocity;

  /// Radians per second, world space.
  final Vec3 angularVelocity;

  /// A resting body the solver skips until something touches it.
  final bool sleeping;

  double get speed => linearVelocity.length;

  @override
  String toString() => 'BodyState(v: $linearVelocity, ω: $angularVelocity${sleeping ? ', sleeping' : ''})';
}

/// One hit of a physics query, nearest first within a query.
class PhysicsHit {
  const PhysicsHit({required this.entity, required this.point, required this.normal, required this.distance});

  factory PhysicsHit.fromRecord(gen.PhysicsHitRecord r) =>
      PhysicsHit(entity: r.entity, point: r.point, normal: r.normal, distance: r.distance);

  /// The entity whose collider was hit.
  final int entity;

  /// World-space hit point (for an overlap: the collider's origin).
  final Vec3 point;

  /// Surface normal at [point] (zero for an overlap).
  final Vec3 normal;

  /// Along the ray / cast direction (0 for an overlap).
  final double distance;

  @override
  String toString() => 'PhysicsHit(0x${entity.toRadixString(16)} at $point n=$normal d=$distance)';
}
