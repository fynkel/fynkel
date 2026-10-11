// The physics a world has (docs/stage-5.md §3.1): a WorldSystem made the
// first time a collider or a query needs it, holding the native world, the
// colliders by key, and the buffers they follow their objects through.

import 'dart:ffi';
import 'dart:math' as math;

import 'package:ffi/ffi.dart';
import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';

import 'bindings.g.dart';
import 'raw.dart';

part 'colliders.dart';
part 'queries.dart';

/// A world's physics, as Unity's `Physics`: the queries ([raycast] and the
/// rest) and the settings. Every object and behaviour reaches it as
/// `physics`, and the world as `world.physics`; it's made the first time
/// it's asked for, so a game never sets it up.
final class Physics extends WorldSystem {
  Physics._(super.world) {
    _world = _raw.pointer;
  }

  final _raw = RawPhysics();
  late final Pointer<FpWorld> _world;
  var _disposed = false;

  /// Whether queries hit triggers when they're asked to with
  /// [QueryTriggers.useGlobal], the default (Unity's
  /// `Physics.queriesHitTriggers`). True unless set.
  bool queriesHitTriggers = true;

  /// Whether every collider is drawn, with [World.debug]'s lines, each
  /// frame: green for solid ones, yellow for triggers.
  bool drawColliders = false;

  // The colliders by the key they were added to the native world with, and
  // the keys that are free again.
  final _byKey = <Collider?>[];
  final _freeKeys = <int>[];

  // The colliders in the native world, and, in the same order, their
  // objects' ids, their own ids and room for their objects' transforms:
  // what following them reads and sends in one call each.
  final _colliders = <Collider>[];
  Pointer<Uint64> _objectIds = nullptr;
  Pointer<Uint64> _colliderIds = nullptr;
  Pointer<Float> _transforms = nullptr;
  Pointer<FpTransform> _transformsAsStructs = nullptr;
  var _capacity = 0;
  var _listChanged = false;
  var _seenVersion = -1;

  // Scratch for the calls.
  final _id = malloc<Uint64>();
  final _count = malloc<Uint32>();
  final _flag = malloc<Uint32>();
  final _spec = malloc<FpColliderSpec>();
  final _transform = malloc<FpTransform>();
  // The queries' are written and read float by float, through the
  // pointers made here once, and no double is passed to a call: in the JIT
  // a struct's `.ref` makes an object each time, and so does a double
  // passed to a call it doesn't inline, and a query that misses makes none.
  // Laid out as the C header has them: an FpRay is floats 0–2 the origin,
  // 3–5 the direction and 6 the max distance, and an FpSweep its last four;
  // an FpQueryShape is the kind, then floats 1–3 a, 4–6 b, 7 the radius and
  // 8–11 the rotation; an FpHit two 64-bit ids, then floats 4–6 the point,
  // 7–9 the normal and 10 the distance, 12 floats in all.
  final _shape = malloc<FpQueryShape>();
  late final _shapeFloats = _shape.cast<Float>();
  late final _shapeKind = _shape.cast<Uint32>();
  final _vectors = malloc<Float>(7);
  late final _ray = _vectors.cast<FpRay>();
  late final _sweep = (_vectors + 3).cast<FpSweep>();
  late final _point = _vectors.cast<FpVec3>();
  late final _closest = (_vectors + 3).cast<FpVec3>();
  final _hit = malloc<FpHit>();
  late final _hitFloats = _hit.cast<Float>();
  late final _hitIds = _hit.cast<Uint64>();
  final _drawn = malloc<FpShape>();
  final _drawnPose = malloc<FpPose>();
  Pointer<FpHit> _hits = nullptr;
  var _hitsCapacity = 0;
  Pointer<FpFound> _found = nullptr;
  var _foundCapacity = 0;

  /// How many colliders there are, enabled on shown objects.
  int get colliderCount => _colliders.length;

  @override
  void afterLateUpdate() {
    super.afterLateUpdate();
    if (drawColliders && world.debug.enabled) _draw();
  }

  @override
  void dispose() {
    super.dispose();
    _disposed = true;
    for (final c in _colliders) {
      c._physics = null;
      c._id = 0;
    }
    _colliders.clear();
    _raw.dispose();
    for (final p in [_id, _count, _flag, _spec, _transform, _shape, _vectors, _hit]) {
      malloc.free(p);
    }
    for (final p in [_drawn, _drawnPose, _hits, _found, _objectIds, _colliderIds, _transforms]) {
      if (p != nullptr) malloc.free(p);
    }
  }

  // --- Colliders -------------------------------------------------------------

  void _add(Collider collider) {
    final object = collider.gameObject;
    final key = _freeKeys.isNotEmpty ? _freeKeys.removeLast() : (_byKey..add(null)).length - 1;
    collider._layer = object.layer;
    collider._writeSpec(_spec.ref);
    final Transform(:position, :rotation, :scale) = object.worldTransform;
    final t = _transform.ref;
    t.position.set(position);
    t.rotation.set(rotation);
    t.scale.set(scale);
    try {
      checkPhysics(fp_static_collider_add(_world, key, _spec.ref, t, _id));
    } catch (_) {
      _freeKeys.add(key);
      rethrow;
    }
    _byKey[key] = collider;
    collider
      .._id = _id.value
      .._key = key
      .._index = _colliders.length;
    _colliders.add(collider);
    _listChanged = true;
  }

  void _remove(Collider collider) {
    if (_disposed || collider._id == 0) return;
    final id = collider._id;
    collider._id = 0;
    _byKey[collider._key] = null;
    _freeKeys.add(collider._key);
    final last = _colliders.removeLast();
    if (!identical(last, collider)) {
      _colliders[collider._index] = last;
      last._index = collider._index;
    }
    _listChanged = true;
    checkPhysics(fp_collider_remove(_world, id));
  }

  void _respec(Collider collider) {
    if (_disposed || collider._id == 0) return;
    collider._writeSpec(_spec.ref);
    checkPhysics(fp_collider_spec_set(_world, collider._id, _spec.ref));
  }

  /// Puts every collider where its object is now, and on its layer: before
  /// each query, and each step. Nothing to do while no transform, parent or
  /// layer was set since the last time.
  void _follow() {
    if (!_listChanged && world.transformVersion == _seenVersion) return;
    _seenVersion = world.transformVersion;
    final colliders = _colliders;
    final count = colliders.length;
    if (_listChanged) {
      _listChanged = false;
      if (count > _capacity) {
        for (final p in [_objectIds, _colliderIds, _transforms]) {
          if (p != nullptr) malloc.free(p);
        }
        _capacity = count * 2;
        _objectIds = malloc<Uint64>(_capacity);
        _colliderIds = malloc<Uint64>(_capacity);
        _transforms = malloc<Float>(_capacity * 10);
        _transformsAsStructs = _transforms.cast();
      }
      for (var i = 0; i < count; i++) {
        final c = colliders[i];
        _objectIds[i] = c.gameObject.id;
        _colliderIds[i] = c._id;
      }
    }
    for (var i = 0; i < count; i++) {
      final c = colliders[i];
      final layer = c.gameObject.layer;
      if (layer != c._layer) {
        c._layer = layer;
        _respec(c);
      }
    }
    if (count == 0) return;
    world.readWorldTransforms(_objectIds, count, _transforms);
    checkPhysics(fp_colliders_follow(_world, _colliderIds, _transformsAsStructs, count));
  }

  void _draw() {
    _follow();
    final debug = world.debug;
    for (final collider in _colliders) {
      checkPhysics(fp_collider_shape_get(_world, collider._id, _drawn, _drawnPose));
      final color = collider.isTrigger ? _triggerColor : _solidColor;
      final FpShape(:kind, :a, :b, :c) = _drawn.ref;
      final pose = _drawnPose.ref;
      final center = pose.position.get();
      final rotation = pose.rotation.get();
      switch (kind) {
        case FpShapeKind.FP_SHAPE_KIND_BOX:
          debug.box(center, Vec3(a, b, c) * 2, rotation: rotation, color: color);
        case FpShapeKind.FP_SHAPE_KIND_SPHERE:
          debug.sphere(center, a, color: color);
        default:
          _drawCapsule(debug, center, rotation, a, b, color);
      }
    }
  }

  static const _solidColor = Color(0xFF7CDB5A);
  static const _triggerColor = Color(0xFFF2D04B);

  // A capsule along its rotation's Y: a circle around each end, four lines
  // between them, and the half-circles over each end in two planes.
  static void _drawCapsule(Debug debug, Vec3 center, Quat rotation, double halfHeight, double radius, Color color) {
    const segments = 24;
    Vec3 at(Vec3 local) => center + rotation.rotate(local);
    for (final end in [halfHeight, -halfHeight]) {
      final circle = [
        for (var i = 0; i < segments; i++)
          at(Vec3(math.cos(2 * math.pi * i / segments) * radius, end, math.sin(2 * math.pi * i / segments) * radius)),
      ];
      debug.path(circle, closed: true, color: color);
      final sign = end < 0 ? -1.0 : 1.0;
      for (final plane in [0, 1]) {
        debug.path([
          for (var i = 0; i <= segments ~/ 2; i++)
            () {
              final angle = math.pi * i / (segments ~/ 2);
              final (across, up) = (math.cos(angle) * radius, sign * math.sin(angle) * radius);
              return at(plane == 0 ? Vec3(across, end + up, 0) : Vec3(0, end + up, across));
            }(),
        ], color: color);
      }
    }
    for (final (x, z) in [(radius, 0.0), (-radius, 0.0), (0.0, radius), (0.0, -radius)]) {
      debug.line(at(Vec3(x, halfHeight, z)), at(Vec3(x, -halfHeight, z)), color: color);
    }
  }
}

/// `physics` in an object.
extension GameObjectPhysics on GameObject {
  /// Its world's [Physics]. Throws a [StateError] before the object is
  /// added.
  Physics get physics => world.system(Physics._);
}

/// `physics` in a behaviour.
extension BehaviourPhysics on Behaviour {
  /// Its object's world's [Physics]. Throws a [StateError] before the
  /// object is added.
  Physics get physics => world.system(Physics._);
}

/// `world.physics`.
extension WorldPhysics on World {
  /// The world's [Physics].
  Physics get physics => system(Physics._);
}

extension on FpVec3 {
  void set(Vec3 v) => this
    ..x = v.x
    ..y = v.y
    ..z = v.z;

  Vec3 get() => Vec3(x, y, z);
}

extension on FpQuat {
  void set(Quat q) => this
    ..x = q.x
    ..y = q.y
    ..z = q.z
    ..w = q.w;

  Quat get() => Quat(x, y, z, w);
}
