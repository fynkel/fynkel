// The physics world in crates/fynkel_physics_ffi, as plain calls: bodies
// and colliders by id, a step, and what it moved read in one buffer. Later
// steps of docs/stage-5.md build colliders and rigidbodies on it.

import 'dart:ffi';

import 'package:ffi/ffi.dart';
import 'package:fynkel/fynkel.dart';

import 'bindings.g.dart';

/// Why a call into the physics failed. The same codes, in the same order,
/// as `FpStatus` in crates/fynkel_physics_ffi/include/fynkel_physics.h.
enum PhysicsStatus {
  ok,

  /// No body has this id: it was removed, or never added.
  noBody,

  /// No collider has this id.
  noCollider,

  /// A value was out of range, not finite, or not one of the choices.
  invalidValue,

  /// A pointer was null. package:fynkel_physics never passes one, so this
  /// is a bug in it.
  nullPointer,

  /// A buffer had the wrong size.
  wrongSize,

  /// The physics panicked. The message says what happened.
  panic,

  /// An earlier call panicked while changing this world, so it can't be
  /// trusted any more.
  poisoned,
}

/// A call into the physics failed.
final class PhysicsException implements Exception {
  PhysicsException(this.status, this.message);

  final PhysicsStatus status;
  final String message;

  @override
  String toString() => 'PhysicsException(${status.name}): $message';
}

/// What moves a body.
enum BodyKind {
  /// Moved by the simulation: gravity, forces, contacts.
  dynamic,

  /// Moved by the game, through the step; pushes dynamic bodies.
  kinematic,

  /// Doesn't move.
  static,
}

/// Checks once, before the first world, that the physics library is the
/// one these bindings were generated for.
final bool _library = () {
  if (Abi.current() == Abi.androidArm || Abi.current() == Abi.androidIA32) {
    throw UnsupportedError('Fynkel runs on 64-bit devices only, and this one is ${Abi.current()}.');
  }
  final version = fp_abi_version();
  if (version != FP_ABI_VERSION) {
    throw StateError(
      'package:fynkel_physics was built for physics ABI $FP_ABI_VERSION, but the library is ABI $version. '
      'Rebuild: the build hook compiles crates/fynkel_physics_ffi from the same checkout.',
    );
  }
  return true;
}();

void _check(int status) {
  if (status == FpStatus.FP_STATUS_OK) return;
  final message = fp_last_error().cast<Utf8>().toDartString();
  if (status < 0 || status >= PhysicsStatus.values.length) {
    throw PhysicsException(PhysicsStatus.panic, 'unknown status $status: $message');
  }
  throw PhysicsException(PhysicsStatus.values[status], message);
}

/// A physics world in the native library, used through ids: the layer the
/// game's colliders and rigidbodies are built on. A step's moved bodies
/// are read from one buffer ([step], then [movedKey] and the rest), so a
/// step is two calls however many bodies there are.
///
/// Its memory is freed by [dispose], or when nothing reaches it any more.
final class RawPhysics implements Finalizable {
  RawPhysics() {
    _library;
    _world = fp_world_new();
    _finalizer.attach(this, _world.cast(), detach: this);
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FpWorld>)>>(fp_world_free).cast(),
  );

  late Pointer<FpWorld> _world;

  // Scratch for the calls, and the buffer of moved bodies, which grows
  // to the most a step has moved.
  final _id = malloc<Uint64>();
  final _count = malloc<Uint32>();
  final _pose = malloc<FpPose>();
  final _shape = malloc<FpShape>();
  final _vec = malloc<FpVec3>();
  final _ray = malloc<FpRay>();
  final _hit = malloc<FpHit>();
  Pointer<FpMoved> _moved = nullptr;
  var _movedCapacity = 0;
  var _movedCount = 0;

  Pointer<FpWorld> get _live {
    if (_world == nullptr) throw StateError('this physics world was disposed');
    return _world;
  }

  /// Frees the native world now. Using it after this throws.
  void dispose() {
    if (_world == nullptr) return;
    _finalizer.detach(this);
    fp_world_free(_world);
    _world = nullptr;
    for (final p in [_id, _count, _pose, _shape, _vec, _ray, _hit, _moved]) {
      if (p != nullptr) malloc.free(p);
    }
    _moved = nullptr;
  }

  set gravity(Vec3 value) {
    _vec.ref
      ..x = value.x
      ..y = value.y
      ..z = value.z;
    _check(fp_gravity_set(_live, _vec.ref));
  }

  /// Adds a body at [position] and [rotation], with [key] to know it by in
  /// [movedKey], and returns its id.
  int addBody(BodyKind kind, int key, {Vec3 position = Vec3.zero, Quat rotation = Quat.identity}) {
    _writePose(position, rotation);
    _check(fp_body_add(_live, kind.index, key, _pose.ref, _id));
    return _id.value;
  }

  /// Removes a body and the colliders on it.
  void removeBody(int body) => _check(fp_body_remove(_live, body));

  /// Adds a box collider, by half its size on each axis, to [body] (or to
  /// the world, as a static collider, when [body] is 0), and returns its id.
  int addBox(int body, Vec3 halfExtents, {Vec3 position = Vec3.zero, Quat rotation = Quat.identity}) => _addCollider(
    body,
    FpShapeKind.FP_SHAPE_KIND_BOX,
    halfExtents.x,
    halfExtents.y,
    halfExtents.z,
    position,
    rotation,
  );

  /// Adds a sphere collider; see [addBox].
  int addSphere(int body, double radius, {Vec3 position = Vec3.zero, Quat rotation = Quat.identity}) =>
      _addCollider(body, FpShapeKind.FP_SHAPE_KIND_SPHERE, radius, 0, 0, position, rotation);

  /// Adds a capsule collider along Y: [halfHeight] of cylinder between the
  /// two half-spheres' centres on each side of its middle. See [addBox].
  int addCapsule(
    int body,
    double halfHeight,
    double radius, {
    Vec3 position = Vec3.zero,
    Quat rotation = Quat.identity,
  }) => _addCollider(body, FpShapeKind.FP_SHAPE_KIND_CAPSULE, halfHeight, radius, 0, position, rotation);

  int _addCollider(int body, int kind, double a, double b, double c, Vec3 position, Quat rotation) {
    _shape.ref
      ..kind = kind
      ..a = a
      ..b = b
      ..c = c;
    _writePose(position, rotation);
    _check(fp_collider_add(_live, body, _shape.ref, _pose.ref, _id));
    return _id.value;
  }

  void removeCollider(int collider) => _check(fp_collider_remove(_live, collider));

  /// Where a body is now.
  (Vec3, Quat) bodyPose(int body) {
    _check(fp_body_pose_get(_live, body, _pose));
    final FpPose(:position, :rotation) = _pose.ref;
    return (Vec3(position.x, position.y, position.z), Quat(rotation.x, rotation.y, rotation.z, rotation.w));
  }

  /// Puts a body somewhere at once (a teleport), and wakes it.
  void setBodyPose(int body, Vec3 position, Quat rotation) {
    _writePose(position, rotation);
    _check(fp_body_pose_set(_live, body, _pose.ref));
  }

  /// Moves the world on by [dt] seconds and returns how many bodies the
  /// step moved: their keys and poses are [movedKey], [movedPosition] and
  /// [movedRotation] from 0 to one less, until the next step.
  int step(double dt) {
    _check(fp_step(_live, dt, _count));
    return _readMoved();
  }

  /// How far along [direction] from [origin] a ray first hits a collider,
  /// within [maxDistance], or null: every layer, triggers too.
  double? raycast(Vec3 origin, Vec3 direction, double maxDistance) {
    final ray = _ray.ref;
    ray.origin
      ..x = origin.x
      ..y = origin.y
      ..z = origin.z;
    ray.direction
      ..x = direction.x
      ..y = direction.y
      ..z = direction.z;
    ray.max_distance = maxDistance;
    _check(fp_raycast(_live, _ray, 0xFFFFFFFF, 1, _hit));
    final hit = _hit.ref;
    return hit.collider == 0 ? null : hit.distance;
  }

  /// Starts a step of [dt] seconds on a thread of the physics' own, and
  /// returns at once: nothing else may be asked of this world until
  /// [endStep], which waits for it and returns what [step] does. (An
  /// experiment, docs/stage-5.md §6.)
  void beginStep(double dt) => _check(fp_step_begin(_live, dt));

  /// Waits for the step [beginStep] started; returns as [step] does.
  int endStep() {
    _check(fp_step_end(_live, _count));
    return _readMoved();
  }

  int _readMoved() {
    final world = _live;
    final count = _count.value;
    if (count > _movedCapacity) {
      if (_moved != nullptr) malloc.free(_moved);
      _movedCapacity = count * 2;
      _moved = malloc<FpMoved>(_movedCapacity);
    }
    _check(fp_moved_read(world, _moved, count));
    return _movedCount = count;
  }

  /// The key of the [i]th body the last step moved.
  int movedKey(int i) => _movedAt(i).key;

  Vec3 movedPosition(int i) {
    final p = _movedAt(i).pose.position;
    return Vec3(p.x, p.y, p.z);
  }

  Quat movedRotation(int i) {
    final r = _movedAt(i).pose.rotation;
    return Quat(r.x, r.y, r.z, r.w);
  }

  /// How fast the [i]th body the last step moved goes, in m/s.
  Vec3 movedLinearVelocity(int i) {
    final v = _movedAt(i).linear_velocity;
    return Vec3(v.x, v.y, v.z);
  }

  /// How fast the [i]th body the last step moved turns, in rad/s.
  Vec3 movedAngularVelocity(int i) {
    final v = _movedAt(i).angular_velocity;
    return Vec3(v.x, v.y, v.z);
  }

  FpMoved _movedAt(int i) {
    RangeError.checkValidIndex(i, null, 'i', _movedCount);
    return (_moved + i).ref;
  }

  void _writePose(Vec3 p, Quat r) {
    final pose = _pose.ref;
    pose.position
      ..x = p.x
      ..y = p.y
      ..z = p.z;
    pose.rotation
      ..x = r.x
      ..y = r.y
      ..z = r.z
      ..w = r.w;
  }
}

/// For the rest of the package: the native world under a [RawPhysics]. Not
/// exported.
extension RawWorld on RawPhysics {
  Pointer<FpWorld> get pointer => _live;
}

/// For the rest of the package: throws the [PhysicsException] a status
/// other than ok means. Not exported.
void checkPhysics(int status) => _check(status);
