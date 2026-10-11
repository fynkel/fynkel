// The physics a world has (docs/stage-5.md §3.1): a WorldSystem made the
// first time a collider, a rigidbody or a query needs it, holding the
// native world, the colliders and rigidbodies by key, and the buffers they
// follow their objects through.
//
// The step runs between frames (step 4): a fixed step marks it due, the
// end of the frame starts it on the physics' own thread, and the start of
// the next frame collects it and puts the bodies where it left them. Any
// call that needs the native world while it steps waits for it first
// ([Physics._native]).

import 'dart:ffi';
import 'dart:math' as math;

import 'package:ffi/ffi.dart';
import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';

import 'bindings.g.dart';
import 'raw.dart';

part 'colliders.dart';
part 'contacts.dart';
part 'queries.dart';
part 'rigidbody.dart';

/// A world's physics, as Unity's `Physics`: the queries ([raycast] and the
/// rest) and the settings. Every object and behaviour reaches it as
/// `physics`, and the world as `world.physics`; it's made the first time
/// it's asked for, so a game never sets it up.
final class Physics extends WorldSystem {
  Physics._(super.world) {
    _pointer = _raw.pointer;
  }

  final _raw = RawPhysics();
  late final Pointer<FpWorld> _pointer;
  var _disposed = false;

  /// The native world, once the step running between frames, if any, is
  /// collected: every call into it goes through this.
  Pointer<FpWorld> get _world {
    if (_stepping) _collect();
    return _pointer;
  }

  /// Whether the step runs between frames, on a thread of the physics' own
  /// (true, the default), or in the fixed step, as Unity's does.
  ///
  /// Between frames, a step a fixed step makes due starts when the frame
  /// is done and the next frame collects it, before anything else: the
  /// frame's fixed updates and updates see the bodies where the step left
  /// them, and what they do goes into the next step. The frame doesn't
  /// wait for the physics, and a query never waits for the step. A frame
  /// with two fixed steps runs the first in it.
  bool stepBetweenFrames = true;

  /// The world's gravity, in m/s² (Unity's `Physics.gravity`): (0, -9.81,
  /// 0) unless set.
  Vec3 get gravity => _gravity;
  set gravity(Vec3 value) {
    _vec.ref.set(value);
    checkPhysics(fp_gravity_set(_world, _vec.ref));
    _gravity = value;
  }

  var _gravity = const Vec3(0, -9.81, 0);

  /// How many times the solver goes over the contacts each step: more is
  /// stiffer and slower (Unity's `Physics.defaultSolverIterations`, 6).
  int get defaultSolverIterations => _settings().solver_iterations;
  set defaultSolverIterations(int value) {
    if (value < 1) throw ArgumentError.value(value, 'defaultSolverIterations', 'must be 1 or more');
    _settings().solver_iterations = value;
    checkPhysics(fp_settings_set(_world, _setting));
  }

  /// The energy per kilogram under which a body falls asleep (Unity's
  /// `Physics.sleepThreshold`, 0.005).
  double get sleepThreshold => _settings().sleep_threshold;
  set sleepThreshold(double value) {
    if (!(value >= 0) || !value.isFinite) {
      throw ArgumentError.value(value, 'sleepThreshold', 'must be finite and 0 or more');
    }
    _settings().sleep_threshold = value;
    checkPhysics(fp_settings_set(_world, _setting));
  }

  FpSettings _settings() {
    checkPhysics(fp_settings_get(_world, _setting));
    return _setting.ref;
  }

  /// Whether queries hit triggers when they're asked to with
  /// [QueryTriggers.useGlobal], the default (Unity's
  /// `Physics.queriesHitTriggers`). True unless set.
  bool queriesHitTriggers = true;

  /// Whether every collider is drawn, with [World.debug]'s lines, each
  /// frame: green for solid ones, yellow for triggers.
  bool drawColliders = false;

  // The colliders by the key they were added to the native world with, and
  // the keys that are free again; the same for the rigidbodies.
  final _byKey = <Collider?>[];
  final _freeKeys = <int>[];
  final _bodiesByKey = <Rigidbody?>[];
  final _freeBodyKeys = <int>[];

  // The colliders and rigidbodies in the native world, and the rigidbodies
  // drawn between steps.
  final _colliders = <Collider>[];
  final _bodies = <Rigidbody>[];
  final _interpolated = <Rigidbody>[];

  // Room for the moved colliders' objects' ids, their own ids and their
  // objects' transforms: what following them reads and sends in one call
  // each.
  Pointer<Uint64> _objectIds = nullptr;
  Pointer<Uint64> _colliderIds = nullptr;
  Pointer<Float> _transforms = nullptr;
  Pointer<FpTransform> _transformsAsStructs = nullptr;
  var _capacity = 0;
  var _listChanged = false;
  var _seenVersion = -1;

  // The step: running between frames, due (a fixed step asked for it and
  // it hasn't run), and how many have been collected.
  var _stepping = false;
  var _due = false;
  var _steps = 0;

  // The contacts: whether a collider ever listened, whether a step's are
  // still to be told, and the buffers they're read into, with views to
  // read them by word (a struct's `.ref` allocates in the JIT). The keys
  // of the colliders removed since, the first [_leavingBefore] of them
  // before the last step began: those are free once its contacts are told.
  var _listened = false;
  var _contactsDue = false;
  Pointer<FpContact> _contactBuffer = nullptr;
  Pointer<Uint32> _contactUints = nullptr;
  Pointer<Uint64> _contactKeys = nullptr;
  Pointer<Float> _contactFloats = nullptr;
  var _contactCapacity = 0;
  Pointer<FpContactPoint> _pointBuffer = nullptr;
  Pointer<Float> _pointFloats = nullptr;
  var _pointCapacity = 0;
  late final _collision = Collision._(this);
  final _leaving = <int>[];
  var _leavingBefore = 0;

  // Scratch for the calls.
  final _id = malloc<Uint64>();
  final _count = malloc<Uint32>();
  final _pointCount = malloc<Uint32>();
  final _flag = malloc<Uint32>();
  final _spec = malloc<FpColliderSpec>();
  final _transform = malloc<FpTransform>();
  final _bodySpec = malloc<FpBodySpec>();
  final _pose = malloc<FpPose>();
  final _push = malloc<FpPush>();
  final _state = malloc<FpBodyState>();
  final _vec = malloc<FpVec3>();
  final _vec2 = malloc<FpVec3>();
  final _setting = malloc<FpSettings>();
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

  /// How many rigidbodies there are, enabled on shown objects.
  int get rigidbodyCount => _bodies.length;

  @override
  void beforeFrame() {
    super.beforeFrame();
    if (_stepping) _collect();
    _tellContacts();
  }

  // A second fixed step in a frame first runs the step the first left due,
  // so each fixed step's onFixedUpdate sees the step before it done, as in
  // Unity; only the frame's last step runs between frames.
  @override
  void beforeFixedUpdate() {
    super.beforeFixedUpdate();
    if (_due) _stepNow();
  }

  @override
  void afterFixedUpdate() {
    super.afterFixedUpdate();
    if (stepBetweenFrames) {
      _due = true;
    } else {
      _stepNow();
    }
  }

  void _stepNow() {
    _due = false;
    _follow();
    _tellContacts();
    _leavingBefore = _leaving.length;
    _applyMoved(_raw.step(world.time.fixedDelta));
    _tellContacts();
  }

  @override
  void beforeUpdate() {
    super.beforeUpdate();
    if (_interpolated.isNotEmpty) _interpolate();
  }

  @override
  void afterLateUpdate() {
    super.afterLateUpdate();
    if (drawColliders && world.debug.enabled) _draw();
  }

  @override
  void afterFrame() {
    super.afterFrame();
    if (!_due) return;
    _due = false;
    _follow();
    _tellContacts();
    _leavingBefore = _leaving.length;
    // Straight into the native call: a double passed to a Dart function the
    // JIT doesn't inline is allocated.
    checkPhysics(fp_step_begin(_pointer, world.time.fixedDelta));
    _stepping = true;
  }

  @override
  void dispose() {
    super.dispose();
    _disposed = true;
    _stepping = false;
    for (final c in _colliders) {
      c._physics = null;
      c._id = 0;
    }
    for (final b in _bodies) {
      b._physics = null;
      b._id = 0;
    }
    _colliders.clear();
    _bodies.clear();
    _interpolated.clear();
    _raw.dispose();
    for (final p in [_id, _count, _pointCount, _flag, _spec, _transform, _shape, _vectors, _hit]) {
      malloc.free(p);
    }
    for (final p in [_bodySpec, _pose, _push, _state, _vec, _vec2, _setting]) {
      malloc.free(p);
    }
    for (final p in [_drawn, _drawnPose, _hits, _found, _objectIds, _colliderIds, _transforms]) {
      if (p != nullptr) malloc.free(p);
    }
    for (final p in [_contactBuffer, _pointBuffer]) {
      if (p != nullptr) malloc.free(p);
    }
  }

  @override
  void componentAdded(GameObject object, Component component) {
    super.componentAdded(object, component);
    if (component is! Triggers && component is! Collisions) return;
    // The colliders it now listens for: on its object, or on a rigidbody
    // there.
    for (final c in _colliders) {
      if (!identical(c.gameObject, object) && !identical(c._body?.gameObject, object)) continue;
      if (_listensFor(c, c._body) != c._listens) _respec(c);
    }
  }

  // --- Colliders -------------------------------------------------------------

  void _add(Collider collider) {
    final object = collider.gameObject;
    final key = _freeKeys.isNotEmpty ? _freeKeys.removeLast() : (_byKey..add(null)).length - 1;
    collider._layer = object.layer;
    collider._place._walk(object);
    final body = collider._place.body;
    try {
      _addNative(collider, key, body);
    } catch (_) {
      _freeKeys.add(key);
      rethrow;
    }
    _byKey[key] = collider;
    collider
      .._key = key
      .._index = _colliders.length;
    _colliders.add(collider);
    _listChanged = true;
  }

  /// Adds [collider] to the native world with [key], on [body] if any,
  /// where its object is now.
  void _addNative(Collider collider, int key, Rigidbody? body) {
    final object = collider.gameObject;
    collider._writeSpec(_spec.ref, _listensFor(collider, body));
    final Transform(:position, :rotation, :scale) = object.worldTransform;
    final t = _transform.ref;
    t.position.set(position);
    t.rotation.set(rotation);
    t.scale.set(scale);
    checkPhysics(fp_object_collider_add(_world, key, _spec.ref, t, body?._id ?? 0, _id));
    collider
      .._id = _id.value
      .._body = body
      .._contactBody = body;
    body?._colliders.add(collider);
    collider._place.seen();
  }

  void _remove(Collider collider) {
    if (_disposed || collider._id == 0) return;
    final id = collider._id;
    collider._id = 0;
    collider._body?._colliders.remove(collider);
    collider._body = null;
    // Its key is free once the step that ends its contacts is told.
    _leaving.add(collider._key);
    final last = _colliders.removeLast();
    if (!identical(last, collider)) {
      _colliders[collider._index] = last;
      last._index = collider._index;
    }
    _listChanged = true;
    checkPhysics(fp_collider_remove(_world, id));
  }

  /// Puts [collider] on [body] (or none) in the native world: out, and in
  /// again, where its object is now.
  void _reattach(Collider collider, Rigidbody? body) {
    checkPhysics(fp_collider_remove(_world, collider._id));
    collider._id = 0;
    collider._body?._colliders.remove(collider);
    collider._body = null;
    _addNative(collider, collider._key, body);
    _listChanged = true;
  }

  void _respec(Collider collider) {
    if (_disposed || collider._id == 0) return;
    collider._writeSpec(_spec.ref, _listensFor(collider, collider._body));
    checkPhysics(fp_collider_spec_set(_world, collider._id, _spec.ref));
  }

  /// Brings the native world up to what the game did since the last time:
  /// rigidbodies whose objects the game moved are put there (teleported),
  /// colliders whose objects moved follow them, and each is on its object's
  /// layer and on the nearest rigidbody. Before each step and each query;
  /// nothing to do while no transform, parent or layer was set since.
  void _follow() {
    if (_stepping) _collect();
    if (!_listChanged && world.transformVersion == _seenVersion) return;
    _listChanged = false;
    final bodies = _bodies;
    for (var i = 0; i < bodies.length; i++) {
      final body = bodies[i];
      if (body._place.moved(world)) _teleport(body);
    }
    final colliders = _colliders;
    final count = colliders.length;
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
    var moved = 0;
    for (var i = 0; i < colliders.length; i++) {
      final c = colliders[i];
      final layer = c.gameObject.layer;
      if (layer != c._layer) {
        c._layer = layer;
        _respec(c);
      }
      final place = c._place;
      if (!place.moved(world)) continue;
      if (place.body != c._body) {
        _reattach(c, place.body);
        continue;
      }
      place.seen();
      _objectIds[moved] = c.gameObject.id;
      _colliderIds[moved] = c._id;
      moved++;
    }
    _seenVersion = world.transformVersion;
    if (moved == 0) return;
    world.readWorldTransforms(_objectIds, moved, _transforms);
    checkPhysics(fp_colliders_follow(_world, _colliderIds, _transformsAsStructs, moved));
  }

  // --- Rigidbodies -----------------------------------------------------------

  void _addBody(Rigidbody body) {
    final object = body.gameObject;
    final key = _freeBodyKeys.isNotEmpty ? _freeBodyKeys.removeLast() : (_bodiesByKey..add(null)).length - 1;
    body._place._walk(object);
    body._writeSpec(_bodySpec.ref);
    final Transform(:position, :rotation) = object.worldTransform;
    _writePose(position, rotation);
    try {
      checkPhysics(fp_rigidbody_add(_world, key, _bodySpec, _pose, _id));
    } catch (_) {
      _freeBodyKeys.add(key);
      rethrow;
    }
    body._place.seen();
    _bodiesByKey[key] = body;
    body
      .._id = _id.value
      .._key = key
      .._index = _bodies.length
      .._reset(position, rotation);
    _bodies.add(body);
    if (body._interpolation != RigidbodyInterpolation.none) _interpolated.add(body);
    if (body._startLinear != Vec3.zero || body._startAngular != Vec3.zero) {
      body._setVelocities(body._startLinear, body._startAngular);
      body
        .._startLinear = Vec3.zero
        .._startAngular = Vec3.zero;
    }
    // The colliders on its object and below it, down to the next
    // rigidbody, move onto it.
    for (final c in [..._colliders]) {
      if (!identical(c.gameObject, object) && !c._place.isBelow(object)) continue;
      c._place._walk(c.gameObject);
      if (c._place.body != c._body) _reattach(c, c._place.body);
    }
  }

  void _removeBody(Rigidbody body) {
    if (_disposed || body._id == 0) return;
    // Its colliders go to the next rigidbody up, or none, first: removing
    // a body removes the colliders on it.
    final id = body._id;
    for (final c in [...body._colliders]) {
      c._place._walk(c.gameObject, without: body);
      _reattach(c, c._place.body);
    }
    body._id = 0;
    _bodiesByKey[body._key] = null;
    _freeBodyKeys.add(body._key);
    final last = _bodies.removeLast();
    if (!identical(last, body)) {
      _bodies[body._index] = last;
      last._index = body._index;
    }
    _interpolated.remove(body);
    checkPhysics(fp_body_remove(_world, id));
  }

  void _respecBody(Rigidbody body) {
    if (_disposed || body._id == 0) return;
    body._writeSpec(_bodySpec.ref);
    checkPhysics(fp_rigidbody_spec_set(_world, body._id, _bodySpec));
    final interpolated = body._interpolation != RigidbodyInterpolation.none;
    if (interpolated != _interpolated.contains(body)) {
      interpolated ? _interpolated.add(body) : _interpolated.remove(body);
    }
  }

  /// Puts [body] where the game put its object, at once.
  void _teleport(Rigidbody body) {
    final Transform(:position, :rotation) = body.gameObject.worldTransform;
    _writePose(position, rotation);
    checkPhysics(fp_body_pose_set(_world, body._id, _pose.ref));
    body
      .._reset(position, rotation)
      .._place.seen();
  }

  /// Runs the step started between frames to its end, and puts the bodies
  /// where it left them.
  void _collect() {
    _stepping = false;
    _applyMoved(_raw.endStep());
  }

  /// Takes the poses and velocities of the [count] bodies the last step
  /// moved, and puts those not drawn between steps where they are.
  void _applyMoved(int count) {
    _steps++;
    _contactsDue = true;
    final raw = _raw;
    for (var i = 0; i < count; i++) {
      final key = raw.movedKey(i);
      if (key >= _bodiesByKey.length) continue;
      final body = _bodiesByKey[key];
      if (body == null) continue;
      body._stepped(
        raw.movedPosition(i),
        raw.movedRotation(i),
        raw.movedLinearVelocity(i),
        raw.movedAngularVelocity(i),
        _steps,
      );
      if (body._interpolation == RigidbodyInterpolation.none) body._put(body._p1, body._r1);
    }
  }

  /// What [collider] listens for, on [body]: its object's callbacks and
  /// the rigidbody's object's. A collider that listens is reported.
  int _listensFor(Collider collider, Rigidbody? body) {
    final object = collider.gameObject;
    var listens = _listensOf(object);
    if (body != null && !identical(body.gameObject, object)) listens |= _listensOf(body.gameObject);
    if (listens != 0) _listened = true;
    return listens;
  }

  /// Tells the last step's contacts, if they aren't yet, then frees the
  /// keys of the colliders removed before it.
  void _tellContacts() {
    if (!_contactsDue) return;
    _contactsDue = false;
    if (_listened) _readContacts();
    final leaving = _leaving;
    final before = _leavingBefore;
    if (before == 0) return;
    for (var i = 0; i < before; i++) {
      final key = leaving[i];
      _byKey[key] = null;
      _freeKeys.add(key);
      checkPhysics(fp_collider_key_forget(_pointer, key));
    }
    leaving.removeRange(0, before);
    _leavingBefore = 0;
  }

  void _readContacts() {
    checkPhysics(
      fp_contacts_read(_pointer, _contactBuffer, _contactCapacity, _pointBuffer, _pointCapacity, _count, _pointCount),
    );
    final count = _count.value;
    if (count == 0) return;
    if (count > _contactCapacity || _pointCount.value > _pointCapacity) {
      _growContacts(count, _pointCount.value);
      checkPhysics(
        fp_contacts_read(_pointer, _contactBuffer, _contactCapacity, _pointBuffer, _pointCapacity, _count, _pointCount),
      );
    }
    final uints = _contactUints;
    final keys = _contactKeys;
    final byKey = _byKey;
    for (var i = 0; i < count; i++) {
      final a = byKey[keys[i * 7 + 1]];
      final b = byKey[keys[i * 7 + 2]];
      if (a == null || b == null) continue;
      final kind = uints[i * _contactWords];
      final trigger = uints[i * _contactWords + 1] != 0;
      _tell(kind, trigger, i, a, b, false);
      _tell(kind, trigger, i, b, a, true);
    }
  }

  void _growContacts(int count, int points) {
    if (count > _contactCapacity) {
      if (_contactBuffer != nullptr) malloc.free(_contactBuffer);
      _contactCapacity = count * 2;
      _contactBuffer = malloc<FpContact>(_contactCapacity);
      _contactUints = _contactBuffer.cast();
      _contactKeys = _contactBuffer.cast();
      _contactFloats = _contactBuffer.cast();
    }
    if (points > _pointCapacity) {
      if (_pointBuffer != nullptr) malloc.free(_pointBuffer);
      _pointCapacity = points * 2;
      _pointBuffer = malloc<FpContactPoint>(_pointCapacity);
      _pointFloats = _pointBuffer.cast();
    }
  }

  /// Tells [own]'s side of contact [index] with [other]: its object, then
  /// its rigidbody's object, each with its behaviours. [flipped] when
  /// [own] is the contact's b, whose vectors point the other way.
  void _tell(int kind, bool trigger, int index, Collider own, Collider other, bool flipped) {
    if (!trigger) {
      _collision
        .._collider = other
        .._own = own
        .._index = index
        .._flipped = flipped;
    }
    final object = own.gameObject;
    _tellObject(object, kind, trigger, other);
    final body = own._contactBody?.gameObject;
    if (body != null && !identical(body, object)) _tellObject(body, kind, trigger, other);
  }

  void _tellObject(GameObject object, int kind, bool trigger, Collider other) {
    // A hidden object's colliders are gone, and it hears nothing of it.
    if (object.isDestroyed || (kind == FpContactKind.FP_CONTACT_KIND_EXIT && !object.activeInHierarchy)) return;
    _call(object, object, kind, trigger, other);
    for (var i = 0; i < object.componentCount; i++) {
      final c = object.componentAt(i);
      if (!c.isRemoved) _call(object, c, kind, trigger, other);
    }
  }

  void _call(GameObject object, Object target, int kind, bool trigger, Collider other) {
    try {
      if (trigger) {
        if (target is! Triggers) return;
        switch (kind) {
          case FpContactKind.FP_CONTACT_KIND_ENTER:
            target.onTriggerEnter(other);
          case FpContactKind.FP_CONTACT_KIND_STAY:
            target.onTriggerStay(other);
          default:
            target.onTriggerExit(other);
        }
      } else {
        if (target is! Collisions) return;
        switch (kind) {
          case FpContactKind.FP_CONTACT_KIND_ENTER:
            target.onCollisionEnter(_collision);
          case FpContactKind.FP_CONTACT_KIND_STAY:
            target.onCollisionStay(_collision);
          default:
            target.onCollisionExit(_collision);
        }
      }
    } catch (error, stack) {
      world.report(object, error, stack);
    }
  }

  /// The vector at float [at] of contact [index], turned round if
  /// [flipped].
  Vec3 _contactVector(int index, int at, bool flipped) {
    final f = _contactFloats;
    final i = index * _contactWords + at;
    final s = flipped ? -1.0 : 1.0;
    return Vec3(f[i] * s, f[i + 1] * s, f[i + 2] * s);
  }

  /// Puts each interpolated body between where the last two steps left it,
  /// as far as the frame is towards the next step; extrapolated ones ahead
  /// of the last, by its velocity. While a step is due and not run yet,
  /// its bodies are a step behind, so they go a step further on.
  void _interpolate() {
    final time = world.time;
    for (var i = 0; i < _interpolated.length; i++) {
      final body = _interpolated[i];
      if (body._movedAt != _steps) {
        // Asleep, or not moved: where the last step left it, once.
        if (!body._resting) {
          body._resting = true;
          body._put(body._p1, body._r1);
        }
        continue;
      }
      final alpha = time.fixedAlpha;
      final delta = time.fixedDelta;
      final extrapolate = body._interpolation == RigidbodyInterpolation.extrapolate;
      if (_due || extrapolate) {
        final ahead = (alpha + (_due && extrapolate ? 1 : 0)) * delta;
        body._put(body._p1 + body._v1 * ahead, _turned(body._r1, body._w1, ahead));
      } else {
        body._put(Vec3.lerp(body._p0, body._p1, alpha), Quat.slerp(body._r0, body._r1, alpha));
      }
    }
  }

  /// [rotation] turned at [spin] radians a second, about the world's axes,
  /// for [seconds].
  static Quat _turned(Quat rotation, Vec3 spin, double seconds) {
    final speed = spin.length;
    if (speed < 1e-9) return rotation;
    return (Quat.axisAngle(spin / speed, speed * seconds) * rotation).normalized;
  }

  void _writePose(Vec3 position, Quat rotation) {
    final pose = _pose.ref;
    pose.position.set(position);
    pose.rotation.set(rotation);
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
