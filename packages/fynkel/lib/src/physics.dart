// Physics value types (M3-08, blueprint §9): what a game reads back from
// the Rapier-backed simulation — a body's motion state, a query hit, the
// project's physics settings. Bodies and colliders themselves are the
// native components `RigidBody`/`Collider` (lib/generated/schema.g.dart),
// attached through `GameObject.add`/`set`; forces and queries go through
// `GameObject` and `World`.

import 'entity_ref.dart';
import '../generated/schema.g.dart' as gen;
import '../math.dart';

export '../generated/schema.g.dart' show BodyOp, BodyType, Collider, ColliderShape, PhysicsQueryKind, RigidBody;

/// The project's `physics` settings as the world applied them on load
/// (`LoadReport.physics`).
class PhysicsSettings {
  const PhysicsSettings({
    this.gravity = const Vec3(0, -9.81, 0),
    this.meshColliders = 0,
    this.prefabs = 0,
  });

  factory PhysicsSettings.fromJson(Map<String, Object?> j) {
    final g = (j['gravity'] as List?)?.cast<num>();
    return PhysicsSettings(
      gravity: g == null || g.length != 3 ? const Vec3(0, -9.81, 0) : Vec3(g[0].toDouble(), g[1].toDouble(), g[2].toDouble()),
      meshColliders: (j['meshColliders'] as num?)?.toInt() ?? 0,
      prefabs: (j['prefabs'] as num?)?.toInt() ?? 0,
    );
  }

  /// Metres per second squared.
  final Vec3 gravity;

  /// Mesh colliders registered from the scene's cooked meshes.
  final int meshColliders;

  /// Prefabs registered for the scene's `Emitter`s (M3-10).
  final int prefabs;
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

/// A character controller's held state as the last tick left it (M3-09,
/// [GameObject.characterState]).
class CharacterState {
  const CharacterState(
      {required this.grounded, required this.sliding, required this.velocity, this.collisionFlags = CollisionFlags.none});

  factory CharacterState.fromRecord(gen.CharacterStateRecord r) => CharacterState(
      grounded: r.grounded, sliding: r.sliding, velocity: r.velocity, collisionFlags: r.collisionFlags);

  /// Standing on walkable ground (a slope within `maxSlope`) after the
  /// last move; a jump needs it.
  final bool grounded;

  /// Pressed against a slope steeper than `maxSlope`, sliding down it.
  final bool sliding;

  /// The movement actually applied over the last tick, metres per second
  /// (gravity included; zero when standing still).
  final Vec3 velocity;

  /// Where the last move touched something, [CollisionFlags] bits
  /// (Unity's `CharacterController.collisionFlags`):
  /// `collisionFlags & CollisionFlags.sides != 0` while pushing a wall.
  final int collisionFlags;

  @override
  String toString() => 'CharacterState(${grounded ? 'grounded' : 'airborne'}${sliding ? ', sliding' : ''}, v: $velocity)';
}

/// The bits of a `RigidBody`'s `constraints` (Unity's
/// `RigidbodyConstraints`, the same values, foundation 2.5): a frozen axis
/// does not move, whatever pushes it — `RigidBody(constraints:
/// RigidbodyConstraints.freezeRotation)` keeps a body upright.
abstract final class RigidbodyConstraints {
  static const int none = 0;
  static const int freezePositionX = 2;
  static const int freezePositionY = 4;
  static const int freezePositionZ = 8;
  static const int freezeRotationX = 16;
  static const int freezeRotationY = 32;
  static const int freezeRotationZ = 64;
  static const int freezePosition = freezePositionX | freezePositionY | freezePositionZ;
  static const int freezeRotation = freezeRotationX | freezeRotationY | freezeRotationZ;
  static const int freezeAll = freezePosition | freezeRotation;
}

/// The bits of [CharacterState.collisionFlags] (Unity's `CollisionFlags`,
/// the same values).
abstract final class CollisionFlags {
  static const int none = 0;

  /// Touched on the sides of the capsule (a wall).
  static const int sides = 1;

  /// Touched above the capsule's upper sphere centre (a ceiling).
  static const int above = 2;

  /// Touched below its lower sphere centre (the ground).
  static const int below = 4;
}

/// One hit of a physics query, nearest first within a query.
class PhysicsHit {
  const PhysicsHit({required this.entity, required this.point, required this.normal, required this.distance});

  factory PhysicsHit.fromRecord(gen.PhysicsHitRecord r) =>
      PhysicsHit(entity: r.entity, point: r.point, normal: r.normal, distance: r.distance);

  /// The entity whose collider was hit.
  final EntityRef entity;

  /// World-space hit point (for an overlap: the collider's origin).
  final Vec3 point;

  /// Surface normal at [point] (zero for an overlap).
  final Vec3 normal;

  /// Along the ray / cast direction (0 for an overlap).
  final double distance;

  @override
  String toString() => 'PhysicsHit(0x${entity.toString()} at $point n=$normal d=$distance)';
}
