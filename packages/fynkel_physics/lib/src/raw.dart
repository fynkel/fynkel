// The physics world in crates/fynkel_physics_ffi, as plain calls: bodies
// and colliders by id, a step, and what it moved read in one buffer. Later
// steps of docs/stage-5.md build colliders and rigidbodies on it.

import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:fynkel/fynkel.dart';
import 'package:meta/meta.dart';

import 'bindings.g.dart';
import 'owned_memory.dart';

/// Why a call into the physics failed: `FpStatus` in
/// crates/fynkel_physics_ffi/include/fynkel_physics.h, by its [code]. The
/// ones the engine has too have its numbers.
enum PhysicsStatus {
  /// It worked.
  ok(FpStatus.FP_STATUS_OK),

  /// No body has this id: it was removed, or never added.
  noBody(FpStatus.FP_STATUS_NO_BODY),

  /// No collider has this id.
  noCollider(FpStatus.FP_STATUS_NO_COLLIDER),

  /// No joint has this id: it was removed, broke, or was never added.
  noJoint(FpStatus.FP_STATUS_NO_JOINT),

  /// A value was out of range, not finite, or not one of the choices.
  invalidValue(FpStatus.FP_STATUS_INVALID_VALUE),

  /// A pointer was null. package:fynkel_physics never passes one, so this
  /// is a bug in it.
  nullPointer(FpStatus.FP_STATUS_NULL_POINTER),

  /// A buffer had the wrong size.
  wrongSize(FpStatus.FP_STATUS_WRONG_SIZE),

  /// The physics panicked. The message says what happened.
  panic(FpStatus.FP_STATUS_PANIC),

  /// An earlier call panicked while changing this world, so it can't be
  /// trusted any more.
  poisoned(FpStatus.FP_STATUS_POISONED);

  const PhysicsStatus(this.code);

  /// Its number in the C interface.
  final int code;
}

/// The physics itself failed: a panic, or a world a panic left in doubt.
/// A game's own mistakes throw Dart's errors instead: an [ArgumentError]
/// for a bad value, a [StateError] for a call at the wrong time or on
/// something gone. [status] says which failure; [message] is the
/// physics' own explanation.
final class PhysicsException implements Exception {
  /// The physics failed with [status], saying [message].
  PhysicsException(this.status, this.message);

  /// Which failure it was.
  final PhysicsStatus status;

  /// What the physics says happened.
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

/// [_check] for the step calls, which take no values: the only thing they
/// refuse is a call at the wrong time (a step while one runs, an end with
/// none begun), a [StateError].
void _checkTime(int status) {
  if (status == FpStatus.FP_STATUS_INVALID_VALUE) throw StateError(fp_last_error().cast<Utf8>().toDartString());
  _check(status);
}

void _check(int status) {
  if (status == FpStatus.FP_STATUS_OK) return;
  final message = fp_last_error().cast<Utf8>().toDartString();
  for (final known in PhysicsStatus.values) {
    if (known.code != status) continue;
    switch (known) {
      case PhysicsStatus.invalidValue:
        throw ArgumentError(message);
      case PhysicsStatus.noBody || PhysicsStatus.noCollider || PhysicsStatus.noJoint:
        throw StateError(message);
      default:
        throw PhysicsException(known, message);
    }
  }
  throw PhysicsException(PhysicsStatus.panic, 'unknown status $status: $message');
}

/// A physics world in the native library, used through ids: the layer the
/// game's colliders and rigidbodies are built on. A step's moved bodies
/// are read from one buffer ([step], then [movedKey] and the rest), so a
/// step is two calls however many bodies there are.
///
/// Its memory is freed by [dispose], or when nothing reaches it any more.
final class RawPhysics implements Finalizable {
  /// A new, empty world, with Unity's gravity, (0, -9.81, 0).
  RawPhysics() {
    _library;
    _world = fp_world_new();
    _finalizer.attach(this, _world.cast(), detach: this);
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FpWorld>)>>(fp_world_free).cast(),
  );

  late Pointer<FpWorld> _world;

  // Native memory, freed when this is collected or the isolate ends.
  late final _memory = OwnedMemory(this);

  // Scratch for the calls, and the buffer of moved bodies, which grows
  // to the most a step has moved.
  late final _id = _memory<Uint64>();
  late final _count = _memory<Uint32>();
  late final _pose = _memory<FpPose>();
  late final _shape = _memory<FpShape>();
  late final _vec = _memory<FpVec3>();
  late final _ray = _memory<FpRay>();
  late final _hit = _memory<FpHit>();
  Pointer<FpMoved> _moved = nullptr;
  var _movedCapacity = 0;
  // The same, read as numbers: each moved body is _stride floats, its key
  // the first two as one Uint64 (reading through .ref makes an object for
  // each struct it goes through, which the compiled app keeps: 1,000
  // tumbling bodies made 500 KB a frame).
  Float32List _movedFloats = Float32List(0);
  Uint64List _movedKeys = Uint64List(0);
  static final _stride = sizeOf<FpMoved>() ~/ 4;
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
    // The scratch stays until this is collected (OwnedMemory), so a call
    // made after this throws before it can harm anything.
  }

  /// The world's gravity, in m/s²: (0, -9.81, 0) unless set.
  Vec3 get gravity => _gravity;
  set gravity(Vec3 value) {
    _vec.ref
      ..x = value.x
      ..y = value.y
      ..z = value.z;
    _check(fp_gravity_set(_live, _vec.ref));
    _gravity = value;
  }

  var _gravity = const Vec3(0, -9.81, 0);

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

  /// Removes a collider. Throws a [StateError] when no collider has this
  /// id.
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

  /// How long each [step] is, in seconds: 1/50 at first. Throws an
  /// [ArgumentError] unless it's more than 0 and finite.
  double get stepLength => _stepLength;
  set stepLength(double seconds) {
    _check(fp_step_length_set(_live, seconds));
    _stepLength = seconds;
  }

  // Set once rather than passed to each step: in the JIT, a double passed
  // to a native call is allocated.
  double _stepLength = 1 / 50;

  /// Moves the world on by [stepLength] and returns how many bodies the
  /// step moved: their keys and poses are [movedKey], [movedPosition] and
  /// [movedRotation] from 0 to one less, until the next step.
  int step() {
    _checkTime(fp_step(_live, _count));
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

  /// Starts a step of [stepLength] on a thread of the physics' own, and
  /// returns at once: nothing else may be asked of this world until
  /// [endStep], which waits for it and returns what [step] does. (An
  /// experiment, docs/stage-5.md §6.)
  void beginStep() => _checkTime(fp_step_begin(_live));

  /// Waits for the step [beginStep] started; returns as [step] does.
  int endStep() {
    _checkTime(fp_step_end(_live, _count));
    return _readMoved();
  }

  int _readMoved() {
    final world = _live;
    final count = _count.value;
    if (count > _movedCapacity) {
      if (_moved != nullptr) _memory.free(_moved);
      _movedCapacity = count * 2;
      _moved = _memory<FpMoved>(_movedCapacity);
      _movedFloats = _moved.cast<Float>().asTypedList(_movedCapacity * _stride);
      _movedKeys = _moved.cast<Uint64>().asTypedList(_movedCapacity * _stride ~/ 2);
    }
    _check(fp_moved_read(world, _moved, count));
    return _movedCount = count;
  }

  /// The key of the [i]th body the last step moved.
  int movedKey(int i) {
    RangeError.checkValidIndex(i, null, 'i', _movedCount);
    return _movedKeys[i * _stride ~/ 2];
  }

  /// Where the [i]th body the last step moved is, in the world.
  Vec3 movedPosition(int i) {
    final f = _movedFloats, at = _movedOffset(i);
    return Vec3(f[at + 2], f[at + 3], f[at + 4]);
  }

  /// How the [i]th body the last step moved is turned, in the world.
  Quat movedRotation(int i) {
    final f = _movedFloats, at = _movedOffset(i);
    return Quat(f[at + 5], f[at + 6], f[at + 7], f[at + 8]);
  }

  /// How fast the [i]th body the last step moved goes, in m/s.
  Vec3 movedLinearVelocity(int i) {
    final f = _movedFloats, at = _movedOffset(i);
    return Vec3(f[at + 9], f[at + 10], f[at + 11]);
  }

  /// How fast the [i]th body the last step moved turns, in rad/s.
  Vec3 movedAngularVelocity(int i) {
    final f = _movedFloats, at = _movedOffset(i);
    return Vec3(f[at + 12], f[at + 13], f[at + 14]);
  }

  /// Writes the [i]th moved body's velocities into [into]: how fast it
  /// goes (0 to 2) and turns (3 to 5), with nothing made (for the
  /// package's rigidbodies, which keep them as numbers).
  @internal
  void movedVelocities(int i, Float64List into) {
    final f = _movedFloats, at = _movedOffset(i);
    into
      ..[0] = f[at + 9]
      ..[1] = f[at + 10]
      ..[2] = f[at + 11]
      ..[3] = f[at + 12]
      ..[4] = f[at + 13]
      ..[5] = f[at + 14];
  }

  /// Where the [i]th moved body's floats start in [_movedFloats]: after
  /// its key, the position (3), the rotation (4) and the two velocities.
  int _movedOffset(int i) {
    RangeError.checkValidIndex(i, null, 'i', _movedCount);
    return i * _stride;
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
  /// The native world. Throws a [StateError] once it's disposed.
  Pointer<FpWorld> get pointer => _live;
}

/// For the rest of the package: throws what a status other than ok means,
/// as [_check] does. Not exported.
void checkPhysics(int status) => _check(status);
