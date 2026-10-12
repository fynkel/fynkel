part of 'physics.dart';

// Queries (docs/stage-5.md §3.6), as Unity's Physics has them: rays, shape
// casts and overlaps, on layers, with or without triggers. They see each
// collider where its object is now, moved or not since the last step.

/// Whether a query finds triggers (Unity's `QueryTriggerInteraction`).
enum QueryTriggers {
  /// As [Physics.queriesHitTriggers] says.
  useGlobal,

  /// It goes through them.
  ignore,

  /// It finds them.
  collide,
}

/// Where a query hit a collider (Unity's `RaycastHit`, for casts too).
final class RaycastHit {
  RaycastHit._(this.collider, this.point, this.normal, this.distance);

  final Collider collider;

  /// In the world. Zero for a cast's hit at distance 0 (one the shape
  /// started in).
  final Vec3 point;

  /// The surface's normal there, in the world, unit length, facing out of
  /// it; against the cast for a hit at distance 0.
  final Vec3 normal;

  /// How far the ray or shape went to get there.
  final double distance;

  /// The collider's object.
  GameObject get gameObject => collider.gameObject;

  /// The rigidbody the collider moves with, if any
  /// ([Collider.attachedRigidbody]).
  Rigidbody? get rigidbody => collider.attachedRigidbody;

  @override
  String toString() =>
      'RaycastHit(${collider.runtimeType} on $gameObject at $point, normal $normal, distance $distance)';
}

extension _Queries on Physics {
  int _hitTriggers(QueryTriggers triggers) => switch (triggers) {
    QueryTriggers.useGlobal => queriesHitTriggers ? 1 : 0,
    QueryTriggers.ignore => 0,
    QueryTriggers.collide => 1,
  };

  // These helpers take no doubles: the JIT may not inline them, and a
  // double passed to a call it doesn't inline is allocated. Each query
  // writes its max distance (float 6 of _vectors) and radius (float 7 of
  // _shapeFloats) itself.

  void _writeRay(Vec3 origin, Vec3 direction) {
    _vectors
      ..[0] = origin.x
      ..[1] = origin.y
      ..[2] = origin.z
      ..[3] = direction.x
      ..[4] = direction.y
      ..[5] = direction.z;
  }

  void _sphere(Vec3 center) {
    _shapeKind[0] = FpQueryShapeKind.FP_QUERY_SHAPE_KIND_SPHERE;
    _shapeFloats
      ..[1] = center.x
      ..[2] = center.y
      ..[3] = center.z;
  }

  void _box(Vec3 center, Vec3 halfExtents, Quat orientation) {
    _shapeKind[0] = FpQueryShapeKind.FP_QUERY_SHAPE_KIND_BOX;
    _shapeFloats
      ..[1] = center.x
      ..[2] = center.y
      ..[3] = center.z
      ..[4] = halfExtents.x
      ..[5] = halfExtents.y
      ..[6] = halfExtents.z
      ..[8] = orientation.x
      ..[9] = orientation.y
      ..[10] = orientation.z
      ..[11] = orientation.w;
  }

  void _capsule(Vec3 point1, Vec3 point2) {
    _shapeKind[0] = FpQueryShapeKind.FP_QUERY_SHAPE_KIND_CAPSULE;
    _shapeFloats
      ..[1] = point1.x
      ..[2] = point1.y
      ..[3] = point1.z
      ..[4] = point2.x
      ..[5] = point2.y
      ..[6] = point2.z;
  }

  RaycastHit? _cast(Vec3 direction, LayerMask layers, QueryTriggers triggers) {
    _follow();
    _writeRay(Vec3.zero, direction);
    checkPhysics(fp_cast(_world, _shape, _sweep, layers.value, _hitTriggers(triggers), _hit));
    return _readHit(_hitFloats, _hitIds, 0);
  }

  List<RaycastHit> _castAll(Vec3 direction, LayerMask layers, QueryTriggers triggers) {
    _follow();
    _writeRay(Vec3.zero, direction);
    checkPhysics(fp_cast_all(_world, _shape, _sweep, layers.value, _hitTriggers(triggers), _count));
    return _readHits();
  }

  List<Collider> _overlap(LayerMask layers, QueryTriggers triggers) {
    _follow();
    checkPhysics(fp_overlap(_world, _shape, layers.value, _hitTriggers(triggers), _count));
    final count = _count.value;
    if (count == 0) return const [];
    if (count > _foundCapacity) {
      if (_found != nullptr) _memory.free(_found);
      _foundCapacity = count * 2;
      _found = _memory<FpFound>(_foundCapacity);
    }
    checkPhysics(fp_found_read(_world, _found, count));
    // An FpFound is two 64-bit ids: the collider's, then its key.
    final ids = _found.cast<Uint64>();
    return [for (var i = 0; i < count; i++) _byKey[ids[i * 2 + 1]]!];
  }

  bool _check(LayerMask layers, QueryTriggers triggers) {
    _follow();
    checkPhysics(fp_check(_world, _shape, layers.value, _hitTriggers(triggers), _flag));
    return _flag.value == 1;
  }

  /// The hit [i] FpHits into [floats] and [ids], or null if its collider
  /// is 0.
  RaycastHit? _readHit(Pointer<Float> floats, Pointer<Uint64> ids, int i) {
    if (ids[i * 6] == 0) return null;
    final f = i * 12;
    return RaycastHit._(
      _byKey[ids[i * 6 + 1]]!,
      Vec3(floats[f + 4], floats[f + 5], floats[f + 6]),
      Vec3(floats[f + 7], floats[f + 8], floats[f + 9]),
      floats[f + 10],
    );
  }

  List<RaycastHit> _readHits() {
    final count = _count.value;
    if (count == 0) return const [];
    if (count > _hitsCapacity) {
      if (_hits != nullptr) _memory.free(_hits);
      _hitsCapacity = count * 2;
      _hits = _memory<FpHit>(_hitsCapacity);
    }
    checkPhysics(fp_hits_read(_world, _hits, count));
    final (floats, ids) = (_hits.cast<Float>(), _hits.cast<Uint64>());
    return [for (var i = 0; i < count; i++) _readHit(floats, ids, i)!];
  }
}

/// The queries, on [Physics]. Each takes [LayerMask] `layers` (every layer
/// unless given) and [QueryTriggers] `triggers`, and sees each collider
/// where its object is now. As Unity's, a ray doesn't hit a collider it
/// starts inside, and a cast doesn't hit one it starts in, except the
/// `All` casts, which give those at distance 0.
extension PhysicsQueries on Physics {
  /// The first collider [ray] hits within [maxDistance], or null (Unity's
  /// `Physics.Raycast`).
  RaycastHit? raycast(
    Ray ray, {
    double maxDistance = double.infinity,
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _follow();
    _writeRay(ray.origin, ray.direction);
    _vectors[6] = maxDistance;
    checkPhysics(fp_raycast(_world, _ray, layers.value, _hitTriggers(triggers), _hit));
    return _readHit(_hitFloats, _hitIds, 0);
  }

  /// Every collider [ray] hits within [maxDistance], nearest first
  /// (Unity's `Physics.RaycastAll`, sorted).
  List<RaycastHit> raycastAll(
    Ray ray, {
    double maxDistance = double.infinity,
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _follow();
    _writeRay(ray.origin, ray.direction);
    _vectors[6] = maxDistance;
    checkPhysics(fp_raycast_all(_world, _ray, layers.value, _hitTriggers(triggers), _count));
    return _readHits();
  }

  /// The first collider a sphere of [radius] hits, swept from [ray]'s
  /// origin along it (Unity's `Physics.SphereCast`).
  RaycastHit? sphereCast(
    Ray ray,
    double radius, {
    double maxDistance = double.infinity,
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _sphere(ray.origin);
    _shapeFloats[7] = radius;
    _vectors[6] = maxDistance;
    return _cast(ray.direction, layers, triggers);
  }

  /// Every collider [sphereCast]'s sphere hits, nearest first.
  List<RaycastHit> sphereCastAll(
    Ray ray,
    double radius, {
    double maxDistance = double.infinity,
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _sphere(ray.origin);
    _shapeFloats[7] = radius;
    _vectors[6] = maxDistance;
    return _castAll(ray.direction, layers, triggers);
  }

  /// The first collider a box hits, half [halfExtents] across, its middle
  /// at [center], turned by [orientation], swept along [direction] (Unity's
  /// `Physics.BoxCast`).
  RaycastHit? boxCast(
    Vec3 center,
    Vec3 halfExtents,
    Vec3 direction, {
    Quat orientation = Quat.identity,
    double maxDistance = double.infinity,
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _box(center, halfExtents, orientation);
    _vectors[6] = maxDistance;
    return _cast(direction, layers, triggers);
  }

  /// Every collider [boxCast]'s box hits, nearest first.
  List<RaycastHit> boxCastAll(
    Vec3 center,
    Vec3 halfExtents,
    Vec3 direction, {
    Quat orientation = Quat.identity,
    double maxDistance = double.infinity,
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _box(center, halfExtents, orientation);
    _vectors[6] = maxDistance;
    return _castAll(direction, layers, triggers);
  }

  /// The first collider a capsule hits, its half-spheres' centres at
  /// [point1] and [point2], swept along [direction] (Unity's
  /// `Physics.CapsuleCast`).
  RaycastHit? capsuleCast(
    Vec3 point1,
    Vec3 point2,
    double radius,
    Vec3 direction, {
    double maxDistance = double.infinity,
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _capsule(point1, point2);
    _shapeFloats[7] = radius;
    _vectors[6] = maxDistance;
    return _cast(direction, layers, triggers);
  }

  /// Every collider [capsuleCast]'s capsule hits, nearest first.
  List<RaycastHit> capsuleCastAll(
    Vec3 point1,
    Vec3 point2,
    double radius,
    Vec3 direction, {
    double maxDistance = double.infinity,
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _capsule(point1, point2);
    _shapeFloats[7] = radius;
    _vectors[6] = maxDistance;
    return _castAll(direction, layers, triggers);
  }

  /// The colliders touching or inside a sphere (Unity's
  /// `Physics.OverlapSphere`), in no order.
  List<Collider> overlapSphere(
    Vec3 center,
    double radius, {
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _sphere(center);
    _shapeFloats[7] = radius;
    return _overlap(layers, triggers);
  }

  /// The colliders touching or inside a box (Unity's `Physics.OverlapBox`).
  List<Collider> overlapBox(
    Vec3 center,
    Vec3 halfExtents, {
    Quat orientation = Quat.identity,
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _box(center, halfExtents, orientation);
    return _overlap(layers, triggers);
  }

  /// The colliders touching or inside a capsule (Unity's
  /// `Physics.OverlapCapsule`).
  List<Collider> overlapCapsule(
    Vec3 point1,
    Vec3 point2,
    double radius, {
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _capsule(point1, point2);
    _shapeFloats[7] = radius;
    return _overlap(layers, triggers);
  }

  /// Whether any collider touches or is inside a sphere (Unity's
  /// `Physics.CheckSphere`).
  bool checkSphere(
    Vec3 center,
    double radius, {
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _sphere(center);
    _shapeFloats[7] = radius;
    return _check(layers, triggers);
  }

  /// Whether any collider touches or is inside a box.
  bool checkBox(
    Vec3 center,
    Vec3 halfExtents, {
    Quat orientation = Quat.identity,
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _box(center, halfExtents, orientation);
    return _check(layers, triggers);
  }

  /// Whether any collider touches or is inside a capsule.
  bool checkCapsule(
    Vec3 point1,
    Vec3 point2,
    double radius, {
    LayerMask layers = LayerMask.all,
    QueryTriggers triggers = QueryTriggers.useGlobal,
  }) {
    _capsule(point1, point2);
    _shapeFloats[7] = radius;
    return _check(layers, triggers);
  }
}
