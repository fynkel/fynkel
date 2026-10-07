// What every call into the engine shares: the ABI check, turning a status
// into an exception, and scratch memory for passing structs.

import 'dart:ffi';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'color.dart';
import 'error.dart';
import 'math.dart';

/// Checks once, before the first world, that the engine library is the one
/// these bindings were generated for.
void ensureEngine() => _engine;

final bool _engine = () {
  final version = fy_abi_version();
  if (version != FY_ABI_VERSION) {
    throw StateError(
      'package:fynkel was built for engine ABI $FY_ABI_VERSION, but the engine library is ABI $version. '
      'Rebuild: the build hook compiles crates/fynkel_ffi from the same checkout.',
    );
  }
  return true;
}();

/// Throws a [FynkelException] for any status but OK.
void check(int status) {
  if (status == FyStatus.FY_STATUS_OK) return;
  final message = fy_last_error().cast<Utf8>().toDartString();
  if (status < 0 || status >= FynkelStatus.values.length) {
    throw FynkelException(FynkelStatus.panic, 'unknown status $status: $message');
  }
  throw FynkelException(FynkelStatus.values[status], message);
}

/// One of each struct the calls pass, allocated once per isolate and
/// reused by every call, so a call allocates no native memory. Calls never
/// call back into Dart, so nothing can use the scratch while a call has it.
/// It lives as long as the isolate.
final class Scratch {
  final id = malloc<Uint64>();
  final transform = malloc<FyTransform>();
  final camera = malloc<FyCamera>();
  final light = malloc<FyLight>();
  final meshRenderer = malloc<FyMeshRenderer>();
  final material = malloc<FyMaterial>();
}

final scratch = Scratch();

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
