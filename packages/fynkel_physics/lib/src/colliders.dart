part of 'physics.dart';

// Colliders (docs/stage-5.md §3.3), as Unity's: behaviours whose shape is
// given in their object's space and follows its world position, rotation
// and scale. On a rigidbody's object, or below one, a collider is part of
// the rigidbody's shape; without one it's static, and can still be moved:
// queries find it where it was moved.

/// How two colliders' frictions or bouncinesses make the pair's (Unity's
/// `PhysicsMaterialCombine`). Where the two differ, the later one here
/// wins.
enum PhysicsMaterialCombine { average, minimum, multiply, maximum }

/// What a collider's surface is like (Unity's `PhysicsMaterial`): how it
/// slides and bounces.
final class PhysicsMaterial {
  const PhysicsMaterial({
    this.dynamicFriction = 0.6,
    this.bounciness = 0,
    this.frictionCombine = PhysicsMaterialCombine.average,
    this.bounceCombine = PhysicsMaterialCombine.average,
  });

  /// Unity's default: friction 0.6, no bounce.
  static const standard = PhysicsMaterial();

  /// How hard it is to slide on, 0 (ice) and up. Physics has one friction,
  /// for sliding and for starting to slide.
  final double dynamicFriction;

  /// How much of its speed a body keeps when it bounces off, 0 to 1.
  final double bounciness;

  final PhysicsMaterialCombine frictionCombine;
  final PhysicsMaterialCombine bounceCombine;

  void _check() {
    if (!(dynamicFriction >= 0) || !dynamicFriction.isFinite) {
      throw ArgumentError.value(dynamicFriction, 'dynamicFriction', 'must be finite and 0 or more');
    }
    if (!(bounciness >= 0 && bounciness <= 1)) throw ArgumentError.value(bounciness, 'bounciness', 'must be 0 to 1');
  }
}

/// A shape physics knows an object by: [BoxCollider], [SphereCollider] or
/// [CapsuleCollider]. It's in the world while it's enabled and its object
/// is shown, at its object's world position, rotation and scale, and on
/// its object's layer.
sealed class Collider extends Behaviour {
  Collider({this._center = Vec3.zero, this._isTrigger = false, PhysicsMaterial material = PhysicsMaterial.standard})
    : _material = material {
    material._check();
  }

  /// Its middle, in its object's space (Unity's `center`).
  Vec3 get center => _center;
  set center(Vec3 value) {
    _center = value;
    _changed();
  }

  Vec3 _center;

  /// Whether it's a trigger: queries find it unless they're told not to
  /// ([QueryTriggers]), and (from step 5) it reports what enters it rather
  /// than stopping it.
  bool get isTrigger => _isTrigger;
  set isTrigger(bool value) {
    _isTrigger = value;
    _changed();
  }

  bool _isTrigger;

  /// What its surface is like.
  PhysicsMaterial get material => _material;
  set material(PhysicsMaterial value) {
    value._check();
    _material = value;
    _changed();
  }

  PhysicsMaterial _material;

  /// The rigidbody it's part of: the nearest on its object or above it, or
  /// null while it's static (or not in the world).
  Rigidbody? get attachedRigidbody {
    _physics?._follow();
    return _body;
  }

  // Its world's physics while it's enabled, its id and key there, where it
  // is in the physics' list, the layer it was given, the rigidbody it's on,
  // where its object is, and whether it hadn't moved when its rigidbody
  // last put its object.
  Physics? _physics;
  int _id = 0;
  int _key = 0;
  int _index = 0;
  int _layer = 0;
  Rigidbody? _body;
  final _place = _Place();
  var _clean = false;

  /// The point on it nearest [point], in the world: [point] itself when
  /// it's inside. Throws a [StateError] while it isn't in the world.
  Vec3 closestPoint(Vec3 point) {
    final physics = _inWorld('closestPoint');
    physics._follow();
    physics._writeRay(point, Vec3.zero);
    checkPhysics(fp_closest_point(physics._world, _id, physics._point, physics._closest));
    final out = physics._vectors;
    return Vec3(out[3], out[4], out[5]);
  }

  /// Where [ray] hits this collider within [maxDistance], or null: only
  /// this one, whatever is in front of it, and not from inside it (Unity's
  /// `Collider.Raycast`). Throws a [StateError] while it isn't in the world.
  PhysicsHit? raycast(Ray ray, {double maxDistance = double.infinity}) {
    final physics = _inWorld('raycast');
    physics._follow();
    physics._writeRay(ray.origin, ray.direction);
    physics._vectors[6] = maxDistance;
    checkPhysics(fp_collider_raycast(physics._world, _id, physics._ray, physics._hit));
    return physics._readHit(physics._hitFloats, physics._hitIds, 0);
  }

  Physics _inWorld(String what) {
    if (_physics case final physics? when _id != 0) return physics;
    throw StateError('$runtimeType.$what: it isn\'t in the world (not enabled, or its object isn\'t shown)');
  }

  @override
  void onEnable() {
    super.onEnable();
    final physics = gameObject.physics;
    _physics = physics;
    physics._add(this);
  }

  @override
  void onDisable() {
    super.onDisable();
    _physics?._remove(this);
    _physics = null;
  }

  void _changed() {
    _checkShape();
    _physics?._respec(this);
  }

  /// Throws an [ArgumentError] for a size it can't have.
  void _checkShape();

  void _writeSpec(FpColliderSpec spec) {
    spec.center.set(_center);
    spec
      ..is_trigger = _isTrigger ? 1 : 0
      ..layer = _layer;
    spec.material
      ..friction = _material.dynamicFriction
      ..bounciness = _material.bounciness
      ..friction_combine = _material.frictionCombine.index
      ..bounce_combine = _material.bounceCombine.index;
    _writeShape(spec);
  }

  void _writeShape(FpColliderSpec spec);

  static void _positive(String name, double value) {
    if (!(value > 0) || !value.isFinite) throw ArgumentError.value(value, name, 'must be finite and more than 0');
  }
}

/// A box, [size] across on each axis of its object's space (Unity's
/// `BoxCollider`); 1 × 1 × 1 unless given, the size of `Mesh.cube()`.
final class BoxCollider extends Collider {
  BoxCollider({this._size = Vec3.one, super.center, super.isTrigger, super.material}) {
    _checkShape();
  }

  Vec3 get size => _size;
  set size(Vec3 value) {
    _size = value;
    _changed();
  }

  Vec3 _size;

  @override
  void _checkShape() {
    Collider._positive('size.x', _size.x);
    Collider._positive('size.y', _size.y);
    Collider._positive('size.z', _size.z);
  }

  @override
  void _writeShape(FpColliderSpec spec) {
    spec.kind = FpColliderKind.FP_COLLIDER_KIND_BOX;
    spec.size.set(_size);
  }
}

/// A sphere of [radius] around [center] (Unity's `SphereCollider`); 0.5
/// unless given, the size of `Mesh.sphere()`. Its object's largest scale
/// scales it: it stays a sphere.
final class SphereCollider extends Collider {
  SphereCollider({this._radius = 0.5, super.center, super.isTrigger, super.material}) {
    _checkShape();
  }

  double get radius => _radius;
  set radius(double value) {
    _radius = value;
    _changed();
  }

  double _radius;

  @override
  void _checkShape() => Collider._positive('radius', _radius);

  @override
  void _writeShape(FpColliderSpec spec) {
    spec.kind = FpColliderKind.FP_COLLIDER_KIND_SPHERE;
    spec.size
      ..x = _radius
      ..y = 0
      ..z = 0;
  }
}

/// The axis a [CapsuleCollider] runs along, in its object's space.
enum CapsuleDirection { x, y, z }

/// A capsule: a cylinder with a half-sphere on each end, [height] from end
/// to end along [direction], of [radius] (Unity's `CapsuleCollider`). Its
/// object's scale along [direction] scales its height, and the larger of
/// the other two its radius. A height of less than twice the radius makes
/// it a sphere.
final class CapsuleCollider extends Collider {
  CapsuleCollider({
    this._radius = 0.5,
    this._height = 2,
    this._direction = CapsuleDirection.y,
    super.center,
    super.isTrigger,
    super.material,
  }) {
    _checkShape();
  }

  double get radius => _radius;
  set radius(double value) {
    _radius = value;
    _changed();
  }

  double _radius;

  double get height => _height;
  set height(double value) {
    _height = value;
    _changed();
  }

  double _height;

  CapsuleDirection get direction => _direction;
  set direction(CapsuleDirection value) {
    _direction = value;
    _changed();
  }

  CapsuleDirection _direction;

  @override
  void _checkShape() {
    Collider._positive('radius', _radius);
    Collider._positive('height', _height);
  }

  @override
  void _writeShape(FpColliderSpec spec) {
    spec
      ..kind = FpColliderKind.FP_COLLIDER_KIND_CAPSULE
      ..direction = _direction.index;
    spec.size
      ..x = _radius
      ..y = _height
      ..z = 0;
  }
}
