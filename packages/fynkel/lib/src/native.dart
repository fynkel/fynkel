// What every call into the engine shares: the ABI check, turning a status
// into an exception, and scratch memory for passing structs.
// Not exported, so its members need no docs:
// ignore_for_file: public_member_api_docs

import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'color.dart';
import 'error.dart';
import 'math.dart';

/// Checks once, before the first world, that the engine library is the one
/// these bindings were generated for.
void ensureEngine() => _engine;

final bool _engine = () {
  // The build hook leaves the engine out of 32-bit Android builds.
  if (Abi.current() == Abi.androidArm || Abi.current() == Abi.androidIA32) {
    throw UnsupportedError('Fynkel runs on 64-bit devices only, and this one is ${Abi.current()}.');
  }
  final version = fy_abi_version();
  if (version != FY_ABI_VERSION) {
    throw StateError(
      'package:fynkel was built for engine ABI $FY_ABI_VERSION, but the engine library is ABI $version. '
      'Rebuild: the build hook compiles crates/fynkel_ffi from the same checkout.',
    );
  }
  return true;
}();

/// [check] for reading a file: one the engine can't read is a
/// [FormatException], as Dart's own decoders throw.
void checkFile(int status) {
  if (status == FyStatus.FY_STATUS_INVALID_VALUE) {
    throw FormatException(fy_last_error().cast<Utf8>().toDartString());
  }
  check(status);
}

/// A mesh, material or texture can't go while something uses it.
final class InUseError extends StateError {
  InUseError(super.message);
}

/// Throws for any status but OK: what the caller got wrong as Dart's own
/// errors, an [ArgumentError] for a bad value and a [StateError] for
/// something gone or still in use, and the engine's own failures as a
/// [FynkelException].
void check(int status) {
  if (status == FyStatus.FY_STATUS_OK) return;
  final message = fy_last_error().cast<Utf8>().toDartString();
  if (status < 0 || status >= FynkelStatus.values.length) {
    throw FynkelException(FynkelStatus.panic, 'unknown status $status: $message');
  }
  switch (FynkelStatus.values[status]) {
    case FynkelStatus.invalidValue:
      throw ArgumentError(message);
    case FynkelStatus.noObject ||
        FynkelStatus.noComponent ||
        FynkelStatus.noMesh ||
        FynkelStatus.noMaterial ||
        FynkelStatus.noTexture ||
        FynkelStatus.noShader:
      throw StateError(message);
    case FynkelStatus.inUse:
      throw InUseError(message);
    case final other:
      throw FynkelException(other, message);
  }
}

/// One of each struct the calls pass, allocated once per isolate and
/// reused by every call, so a call allocates no native memory. Calls never
/// call back into Dart, so nothing can use the scratch while a call has it.
/// It lives as long as the isolate, except a helper's ([inHelper]).
final class Scratch {
  final id = malloc<Uint64>();
  final transform = malloc<FyTransform>();
  final camera = malloc<FyCamera>();
  final light = malloc<FyLight>();
  final meshRenderer = malloc<FyMeshRenderer>();
  final material = malloc<FyMaterial>();
  final viewStats = malloc<FyViewStats>();
  final count = malloc<Size>();
  final count2 = malloc<Size>();
  final mesh = malloc<Pointer<FyMesh>>();
  final texture = malloc<Pointer<FyTexture>>();
  final shader = malloc<Pointer<FyShader>>();
  final model = malloc<Pointer<FyModel>>();
  final modelCounts = malloc<FyModelCounts>();
  final width = malloc<Uint32>();
  final height = malloc<Uint32>();
  final flag = malloc<Bool>();
  final ray = malloc<FyRay>();
  final rayHit = malloc<FyRayHit>();
  final vec3 = malloc<FyVec3>();
  final number = malloc<Float>();
  // Two numbers for a call that takes them through a pointer.
  final numbers = malloc<Float>(2);
  // The second of them, made once: `numbers + 1` makes a pointer each time.
  late final secondNumber = numbers + 1;
  final animatorState = malloc<FyAnimatorState>();
  final particleSettings = malloc<FyParticleSettings>();
  final particleState = malloc<FyParticleState>();
  final trailSettings = malloc<FyTrailSettings>();

  // Room for lists of ids and for strings, grown when a call needs more.
  var _ids = malloc<Uint64>(_initial);
  var _idCapacity = _initial;
  var _bytes = malloc<Uint8>(_initial * 4);
  var _byteCapacity = _initial * 4;
  static const _initial = 16;

  var _transforms = malloc<FyTransform>(_initial);
  var _transformCapacity = _initial;
  late var _transformFloats = _transforms.cast<Float>();

  /// Frees it all. Nothing may use it after.
  void free() {
    final all = [id, transform, camera, light, meshRenderer, material, viewStats, count, count2, mesh, texture];
    all.addAll([model, modelCounts, width, height, flag, ray, rayHit, vec3, _ids, _bytes, _transforms]);
    all.addAll([number, numbers, animatorState, _events, shader, particleSettings, particleState, _particleEvents]);
    all.add(trailSettings);
    all.forEach(malloc.free);
  }

  int get idCapacity => _idCapacity;
  int get byteCapacity => _byteCapacity;

  /// At least [count] ids.
  Pointer<Uint64> ids([int count = 0]) {
    if (count > _idCapacity) {
      malloc.free(_ids);
      _ids = malloc<Uint64>(count);
      _idCapacity = count;
    }
    return _ids;
  }

  /// At least [count] transforms.
  Pointer<FyTransform> transforms([int count = 0]) {
    if (count > _transformCapacity) {
      malloc.free(_transforms);
      _transforms = malloc<FyTransform>(count);
      _transformFloats = _transforms.cast();
      _transformCapacity = count;
    }
    return _transforms;
  }

  /// [transforms] as floats, ten each: kept, as `cast` makes a new pointer
  /// each call in the JIT.
  Pointer<Float> transformFloats([int count = 0]) {
    transforms(count);
    return _transformFloats;
  }

  var _events = malloc<FyAnimationEvent>(_initial);
  var _eventCapacity = _initial;

  /// At least [count] animation events.
  Pointer<FyAnimationEvent> animationEvents([int count = 0]) {
    if (count > _eventCapacity) {
      malloc.free(_events);
      _events = malloc<FyAnimationEvent>(count);
      _eventCapacity = count;
    }
    return _events;
  }

  var _particleEvents = malloc<FyParticleEvent>(_initial);
  var _particleEventCapacity = _initial;
  // The same memory as 64- and 32-bit words, cast once: reading an event's
  // fields through these makes nothing, where a struct's view would
  // (`FyParticleEvent` is 32 bytes: object, kind, action, plane, count).
  late var particleEventWords = _particleEvents.cast<Uint64>();
  late var particleEventHalves = _particleEvents.cast<Uint32>();

  /// At least [count] particle events.
  Pointer<FyParticleEvent> particleEvents([int count = 0]) {
    if (count > _particleEventCapacity) {
      malloc.free(_particleEvents);
      _particleEvents = malloc<FyParticleEvent>(count);
      _particleEventCapacity = count;
      particleEventWords = _particleEvents.cast();
      particleEventHalves = _particleEvents.cast();
    }
    return _particleEvents;
  }

  /// At least [count] bytes.
  Pointer<Uint8> bytes([int count = 0]) {
    if (count > _byteCapacity) {
      malloc.free(_bytes);
      _bytes = malloc<Uint8>(count);
      _byteCapacity = count;
    }
    return _bytes;
  }

  /// Copies [s] as UTF-8 into [bytes] and returns its length in bytes.
  int utf8(String s) {
    final encoded = const Utf8Encoder().convert(s);
    bytes(encoded.length).asTypedList(encoded.length).setAll(0, encoded);
    return encoded.length;
  }
}

final scratch = Scratch();

/// Runs [work] on a helper isolate, as [Isolate.run] does, and frees the
/// helper's [scratch] before it ends: native memory, which would outlive
/// it otherwise.
Future<T> inHelper<T>(T Function() work) => Isolate.run(() {
  try {
    return work();
  } finally {
    scratch.free();
  }
});

extension WriteVec3 on FyVec3 {
  void write(Vec3 v) => this
    ..x = v.x
    ..y = v.y
    ..z = v.z;
  Vec3 read() => Vec3(x, y, z);
}

extension WriteQuat on FyQuat {
  void write(Quat q) => this
    ..x = q.x
    ..y = q.y
    ..z = q.z
    ..w = q.w;
  Quat read() => Quat(x, y, z, w);
}

extension WriteColor on FyColor {
  void write(Color c) => this
    ..r = c.r
    ..g = c.g
    ..b = c.b
    ..a = c.a;
  Color read() => Color.rgb(r, g, b, a);
}
