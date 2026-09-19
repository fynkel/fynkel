/// Allocation-free transform writes (blueprint §19.2c item 1, carry-over C1).
///
/// [Transform], [Vec3] and [Quat] stay immutable values (ADR-0013) and remain
/// the read surface. Hot paths — one write per moving object per tick — go
/// through a reusable [TransformWriter] or the scalar setters on `GameObject`
/// so that no value object is materialised per write.
library;

import 'dart:math' as math;
import 'dart:typed_data';

import '../generated/schema.g.dart' show Transform;
import '../math.dart';

/// A mutable, reusable transform. Create one per behaviour (or per system)
/// and reuse it every tick; every method mutates in place and returns
/// `this` so writes chain.
class TransformWriter {
  TransformWriter();

  /// Starts from an immutable value.
  TransformWriter.of(Transform t) {
    set(t);
  }

  double px = 0, py = 0, pz = 0;
  double qx = 0, qy = 0, qz = 0, qw = 1;
  double sx = 1, sy = 1, sz = 1;

  TransformWriter set(Transform t) {
    px = t.position.x;
    py = t.position.y;
    pz = t.position.z;
    qx = t.rotation.x;
    qy = t.rotation.y;
    qz = t.rotation.z;
    qw = t.rotation.w;
    sx = t.scale.x;
    sy = t.scale.y;
    sz = t.scale.z;
    return this;
  }

  TransformWriter copyFrom(TransformWriter o) {
    px = o.px;
    py = o.py;
    pz = o.pz;
    qx = o.qx;
    qy = o.qy;
    qz = o.qz;
    qw = o.qw;
    sx = o.sx;
    sy = o.sy;
    sz = o.sz;
    return this;
  }

  TransformWriter identity() {
    px = py = pz = 0;
    qx = qy = qz = 0;
    qw = 1;
    sx = sy = sz = 1;
    return this;
  }

  TransformWriter setPosition(double x, double y, double z) {
    px = x;
    py = y;
    pz = z;
    return this;
  }

  TransformWriter translate(double x, double y, double z) {
    px += x;
    py += y;
    pz += z;
    return this;
  }

  TransformWriter setRotation(double x, double y, double z, double w) {
    qx = x;
    qy = y;
    qz = z;
    qw = w;
    return this;
  }

  /// Rotation of [radians] about the unit axis `(ax, ay, az)`.
  TransformWriter setRotationAxisAngle(double ax, double ay, double az, double radians) {
    final h = radians / 2;
    final s = math.sin(h);
    qx = ax * s;
    qy = ay * s;
    qz = az * s;
    qw = math.cos(h);
    return this;
  }

  /// `rotation = rotation * axisAngle(...)` — applies the new rotation
  /// first, in local space (Hamilton product, as `Quat.operator*`).
  TransformWriter rotateAxisAngle(double ax, double ay, double az, double radians) {
    final h = radians / 2;
    final s = math.sin(h);
    final ox = ax * s, oy = ay * s, oz = az * s, ow = math.cos(h);
    final x = qw * ox + qx * ow + qy * oz - qz * oy;
    final y = qw * oy - qx * oz + qy * ow + qz * ox;
    final z = qw * oz + qx * oy - qy * ox + qz * ow;
    final w = qw * ow - qx * ox - qy * oy - qz * oz;
    qx = x;
    qy = y;
    qz = z;
    qw = w;
    return this;
  }

  TransformWriter setScale(double x, double y, double z) {
    sx = x;
    sy = y;
    sz = z;
    return this;
  }

  TransformWriter setUniformScale(double s) => setScale(s, s, s);

  /// Materialises the immutable value (allocates; not for hot paths).
  Transform toTransform() =>
      Transform(position: Vec3(px, py, pz), rotation: Quat(qx, qy, qz, qw), scale: Vec3(sx, sy, sz));

  /// Writes the 40-byte wire `Transform` (f32 LE: position, rotation, scale).
  void encode(ByteData d, int o) {
    d.setFloat32(o, px, Endian.little);
    d.setFloat32(o + 4, py, Endian.little);
    d.setFloat32(o + 8, pz, Endian.little);
    d.setFloat32(o + 12, qx, Endian.little);
    d.setFloat32(o + 16, qy, Endian.little);
    d.setFloat32(o + 20, qz, Endian.little);
    d.setFloat32(o + 24, qw, Endian.little);
    d.setFloat32(o + 28, sx, Endian.little);
    d.setFloat32(o + 32, sy, Endian.little);
    d.setFloat32(o + 36, sz, Endian.little);
  }

  /// Reads the 40-byte wire `Transform` at [o].
  TransformWriter decode(ByteData d, int o) {
    px = d.getFloat32(o, Endian.little);
    py = d.getFloat32(o + 4, Endian.little);
    pz = d.getFloat32(o + 8, Endian.little);
    qx = d.getFloat32(o + 12, Endian.little);
    qy = d.getFloat32(o + 16, Endian.little);
    qz = d.getFloat32(o + 20, Endian.little);
    qw = d.getFloat32(o + 24, Endian.little);
    sx = d.getFloat32(o + 28, Endian.little);
    sy = d.getFloat32(o + 32, Endian.little);
    sz = d.getFloat32(o + 36, Endian.little);
    return this;
  }

  @override
  String toString() => 'TransformWriter(p=($px, $py, $pz) q=($qx, $qy, $qz, $qw) s=($sx, $sy, $sz))';
}
