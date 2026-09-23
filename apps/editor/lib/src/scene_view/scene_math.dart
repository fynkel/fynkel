// The Scene view's linear algebra: vectors and quaternions as plain lists
// (what the authoring package speaks), the editor camera as an orbit rig
// (pivot, distance, yaw, pitch) that also flies and aligns to a scene
// camera, and the pixel ↔ ray ↔ plane math the picking and the gizmos
// need. Column-major 4×4 matrices throughout, like `transform_math.dart`
// and the runtime (glam `to_cols_array`). No math dependency: the editor
// needs a few dozen functions, not a library.
import 'dart:math' as math;

import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../hierarchy/transform_math.dart';

// ── Vectors ───────────────────────────────────────────────────────────────

typedef Vec3 = List<double>;

Vec3 v3(double x, double y, double z) => [x, y, z];
Vec3 vAdd(Vec3 a, Vec3 b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
Vec3 vSub(Vec3 a, Vec3 b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
Vec3 vScale(Vec3 a, double s) => [a[0] * s, a[1] * s, a[2] * s];
double vDot(Vec3 a, Vec3 b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
Vec3 vCross(Vec3 a, Vec3 b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
double vLength(Vec3 a) => math.sqrt(vDot(a, a));
double vDistance(Vec3 a, Vec3 b) => vLength(vSub(a, b));

/// Unit vector; the input for a zero vector (the caller picks a fallback).
Vec3 vNormalize(Vec3 a) {
  final l = vLength(a);
  return l < 1e-12 ? List.of(a) : vScale(a, 1 / l);
}

Vec3 vMin(Vec3 a, Vec3 b) => [math.min(a[0], b[0]), math.min(a[1], b[1]), math.min(a[2], b[2])];
Vec3 vMax(Vec3 a, Vec3 b) => [math.max(a[0], b[0]), math.max(a[1], b[1]), math.max(a[2], b[2])];

/// `m × [v, 1]` — a point through an affine matrix.
Vec3 transformPoint(List<double> m, Vec3 v) => [
  m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12],
  m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13],
  m[2] * v[0] + m[6] * v[1] + m[10] * v[2] + m[14],
];

/// `m × [v, 0]` — a direction through a matrix (no translation).
Vec3 transformDirection(List<double> m, Vec3 v) => [
  m[0] * v[0] + m[4] * v[1] + m[8] * v[2],
  m[1] * v[0] + m[5] * v[1] + m[9] * v[2],
  m[2] * v[0] + m[6] * v[1] + m[10] * v[2],
];

/// The translation column of a matrix.
Vec3 matTranslation(List<double> m) => [m[12], m[13], m[14]];

/// The columns of the rotation part, scale removed (unit axes).
List<Vec3> matAxes(List<double> m) => [
  vNormalize([m[0], m[1], m[2]]),
  vNormalize([m[4], m[5], m[6]]),
  vNormalize([m[8], m[9], m[10]]),
];

// ── Quaternions (x, y, z, w) ──────────────────────────────────────────────

typedef Quat = List<double>;

const Quat quatIdentity = [0, 0, 0, 1];

Quat quatFromAxisAngle(Vec3 axis, double radians) {
  final a = vNormalize(axis);
  final s = math.sin(radians / 2);
  return [a[0] * s, a[1] * s, a[2] * s, math.cos(radians / 2)];
}

/// `a × b`: apply `b`, then `a`.
Quat quatMul(Quat a, Quat b) => [
  a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
  a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
  a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
  a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
];

Quat quatConjugate(Quat q) => [-q[0], -q[1], -q[2], q[3]];

Quat quatNormalize(Quat q) {
  final n = math.sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
  if (n < 1e-12) return List.of(quatIdentity);
  var out = [q[0] / n, q[1] / n, q[2] / n, q[3] / n];
  if (out[3] < 0) out = [-out[0], -out[1], -out[2], -out[3]];
  return out;
}

Vec3 quatRotate(Quat q, Vec3 v) {
  final r = quatMul(quatMul(q, [v[0], v[1], v[2], 0]), quatConjugate(q));
  return [r[0], r[1], r[2]];
}

/// The rotation whose matrix is [m]'s rotation part (through `Trs`).
Quat quatFromMatrix(List<double> m) => Trs.fromMatrix(m).rotation;

/// The matrix of a rotation alone.
List<double> quatToMatrix(Quat q) => Trs(position: const [0, 0, 0], rotation: q, scale: const [1, 1, 1]).toMatrix();

// ── Rays, planes, projection ──────────────────────────────────────────────

final class Ray {
  const Ray(this.origin, this.dir);

  final Vec3 origin;

  /// Unit direction.
  final Vec3 dir;

  Vec3 at(double t) => vAdd(origin, vScale(dir, t));

  /// Where the ray meets the plane through [point] with [normal]; null when
  /// it runs (nearly) parallel or the plane is behind the ray.
  Vec3? hitPlane(Vec3 point, Vec3 normal) {
    final denom = vDot(dir, normal);
    if (denom.abs() < 1e-9) return null;
    final t = vDot(vSub(point, origin), normal) / denom;
    // Behind the origin, or the origin itself on the plane (a camera at
    // ground level would otherwise "hit" the ground at its own eye).
    if (t <= 1e-9) return null;
    return at(t);
  }

  /// The parameter along the line `point + s × axis` closest to this ray
  /// (the skew-lines closest points); null when the two are parallel.
  double? closestOnLine(Vec3 point, Vec3 axis) {
    final a = vNormalize(axis);
    final w = vSub(origin, point);
    final b = vDot(a, dir);
    final d = vDot(a, w);
    final e = vDot(dir, w);
    final denom = 1 - b * b;
    if (denom.abs() < 1e-9) return null;
    return (d - b * e) / denom;
  }
}

/// The projection matrices the runtime uses (`fynkel_core::projection_matrix`),
/// so a point the editor projects lands where the renderer draws it.
List<double> perspectiveMatrix(double fovY, double aspect, double near, double far) {
  final f = 1 / math.tan(fovY / 2);
  return [
    f / aspect, 0, 0, 0, //
    0, f, 0, 0, //
    0, 0, far / (near - far), -1, //
    0, 0, near * far / (near - far), 0,
  ];
}

List<double> orthographicMatrix(double height, double aspect, double near, double far) {
  final h = height / 2, w = h * aspect;
  return [
    1 / w, 0, 0, 0, //
    0, 1 / h, 0, 0, //
    0, 0, 1 / (near - far), 0, //
    0, 0, near / (near - far), 1,
  ];
}

/// The editor camera as an orbit rig around a pivot — what the user steers
/// (orbit, pan, dolly, fly, focus, frame) and what `.fynkel/editor.json`
/// saves per scene. The camera looks along its −Z; yaw turns about the
/// world Y, pitch about the camera's X, there is no roll (a scene camera
/// with roll aligned to loses it, and [alignedWorld] keeps the exact
/// matrix until the next movement so the picture stays pixel-identical).
final class EditorCamera {
  EditorCamera({
    Vec3? pivot,
    this.distance = 10,
    this.yaw = -0.6,
    this.pitch = -0.5,
    this.orthographic = false,
    this.fovY = 60 * math.pi / 180,
    this.near = 0.1,
    this.far = 1000,
    this.manualExposure,
    this.alignedWorld,
  }) : pivot = pivot ?? [0, 0.5, 0];

  /// The default view: from the front-right, above, at the origin.
  factory EditorCamera.initial() => EditorCamera();

  Vec3 pivot;
  double distance;
  double yaw, pitch;
  bool orthographic;
  double fovY, near, far;

  /// A fixed exposure (EV100) taken from an aligned scene camera with
  /// `exposure_mode: manual`; null is the renderer's automatic exposure.
  /// A lens setting like [fovY]: it survives movements.
  double? manualExposure;

  /// An exact world matrix (a scene camera aligned to), dropped by the
  /// first movement.
  List<double>? alignedWorld;

  static const minDistance = 0.05;
  static const maxPitch = 89.5 * math.pi / 180;

  /// The orthographic view's height in metres: what the perspective view
  /// sees at the pivot, so toggling projections keeps the framing.
  double get orthoHeight => 2 * distance * math.tan(fovY / 2);

  Quat get rotation => quatMul(quatFromAxisAngle([0, 1, 0], yaw), quatFromAxisAngle([1, 0, 0], pitch));

  Vec3 get position {
    final a = alignedWorld;
    if (a != null) return matTranslation(a);
    return vAdd(pivot, quatRotate(rotation, [0, 0, distance]));
  }

  Vec3 get forward => alignedWorld != null ? vScale(matAxes(alignedWorld!)[2], -1) : quatRotate(rotation, [0, 0, -1]);
  Vec3 get right => alignedWorld != null ? matAxes(alignedWorld!)[0] : quatRotate(rotation, [1, 0, 0]);
  Vec3 get up => alignedWorld != null ? matAxes(alignedWorld!)[1] : quatRotate(rotation, [0, 1, 0]);

  /// The camera's world matrix (column-major).
  List<double> get world {
    final a = alignedWorld;
    if (a != null) return List.of(a);
    return Trs(position: position, rotation: rotation, scale: const [1, 1, 1]).toMatrix();
  }

  List<double> get view => matInverseAffine(world);

  List<double> projection(double aspect) =>
      orthographic ? orthographicMatrix(orthoHeight, aspect, near, far) : perspectiveMatrix(fovY, aspect, near, far);

  /// What the preview world renders through.
  PreviewCamera toPreview() => PreviewCamera(
    world: world,
    orthographic: orthographic,
    fovY: fovY,
    orthoHeight: orthoHeight,
    near: near,
    far: far,
    manualExposure: manualExposure,
  );

  /// A movement drops the aligned matrix: the rig takes over from where
  /// the aligned camera stood and looked.
  void _unalign() {
    final a = alignedWorld;
    if (a == null) return;
    alignedWorld = null;
    final pos = matTranslation(a);
    final f = vScale(matAxes(a)[2], -1);
    _lookAlong(f);
    pivot = vAdd(pos, vScale(f, distance));
  }

  /// Yaw and pitch from a forward direction.
  void _lookAlong(Vec3 f) {
    final d = vNormalize(f);
    pitch = math.asin(d[1].clamp(-1.0, 1.0)).clamp(-maxPitch, maxPitch);
    yaw = math.atan2(-d[0], -d[2]);
  }

  /// Orbit about the pivot by a pointer delta in pixels.
  void orbit(double dx, double dy) {
    _unalign();
    yaw -= dx * 0.008;
    pitch = (pitch - dy * 0.008).clamp(-maxPitch, maxPitch);
  }

  /// Slide the pivot along the view plane by a pointer delta in pixels; the
  /// point under the pointer at the pivot's depth follows the pointer.
  void pan(double dx, double dy, double viewportHeightPx) {
    _unalign();
    final perPixel = (orthographic ? orthoHeight : 2 * distance * math.tan(fovY / 2)) / math.max(viewportHeightPx, 1);
    pivot = vAdd(pivot, vAdd(vScale(right, -dx * perPixel), vScale(up, dy * perPixel)));
  }

  /// Move towards (positive) or away from the pivot; one wheel notch is
  /// about 10 %. A perspective view never stalls at its pivot: inside
  /// [throughDistance] the camera keeps its pace and pushes the pivot ahead
  /// of it (Unity's zoom-through), so scrolling in walks through the scene.
  void dolly(double steps) {
    _unalign();
    final next = distance * math.exp(-steps * 0.1);
    final floor = math.min(throughDistance, distance);
    if (orthographic || steps <= 0 || next >= floor) {
      distance = math.max(minDistance, next);
      return;
    }
    // The steps left once the distance reaches the floor move at the
    // floor's pace: 10 % of it per notch.
    final beyond = steps - math.log(distance / floor) / 0.1;
    distance = floor;
    pivot = vAdd(pivot, vScale(forward, floor * 0.1 * beyond));
  }

  /// Where a perspective dolly stops closing on the pivot and pushes it.
  static const throughDistance = 0.5;

  /// Fly: move the camera (and its pivot) by metres in its own axes.
  void fly(double forwardM, double rightM, double upM) {
    _unalign();
    final delta = vAdd(vAdd(vScale(forward, forwardM), vScale(right, rightM)), vScale(up, upM));
    pivot = vAdd(pivot, delta);
  }

  /// Look around from where the camera stands (the right-drag): the pivot
  /// swings, the position stays.
  void look(double dx, double dy) {
    _unalign();
    final pos = position;
    yaw -= dx * 0.004;
    pitch = (pitch - dy * 0.004).clamp(-maxPitch, maxPitch);
    pivot = vAdd(pos, vScale(forward, distance));
  }

  /// Frame a sphere: the pivot on its centre, far enough that it fits the
  /// vertical field of view.
  void focus(Vec3 center, double radius) {
    _unalign();
    pivot = List.of(center);
    final r = math.max(radius, 0.25);
    distance = math.max(minDistance, r / math.sin(fovY / 2) * 1.1);
  }

  /// Frame an axis-aligned box.
  void frame(Vec3 min, Vec3 max) {
    final c = vScale(vAdd(min, max), 0.5);
    focus(c, vDistance(min, max) / 2);
  }

  /// Take a scene camera's exact placement and lens. [manualExposure]
  /// replaces the exposure (null: automatic).
  void alignTo(
    List<double> cameraWorld, {
    bool? orthographic,
    double? fovY,
    double? orthoHeight,
    double? near,
    double? far,
    double? manualExposure,
  }) {
    this.manualExposure = manualExposure;
    final m = List.of(cameraWorld);
    // The rig behind the exact matrix, so the next orbit starts from here.
    final pos = matTranslation(m);
    final f = vScale(matAxes(m)[2], -1);
    _lookAlong(f);
    if (fovY != null) this.fovY = fovY;
    if (near != null) this.near = near;
    if (far != null) this.far = far;
    if (orthographic != null) this.orthographic = orthographic;
    if (orthoHeight != null && orthoHeight > 0) distance = orthoHeight / (2 * math.tan(this.fovY / 2));
    pivot = vAdd(pos, vScale(f, distance));
    alignedWorld = m;
  }

  void toggleProjection() => orthographic = !orthographic;

  /// The orientation gizmo's click on an axis end (item 9e): the camera
  /// moves to the [axis] (0 X, 1 Y, 2 Z) side of the pivot — [positive] or
  /// not — and looks back at it, at the same distance. Already looking from
  /// there, it goes to the opposite side. Looking straight down the front
  /// (+Z) is at the bottom of the picture, as seen standing in front;
  /// looking straight up, at the top (the rig has no roll; the pitch stops
  /// just short of vertical, [maxPitch]).
  void lookFrom(int axis, {required bool positive}) {
    if (viewFrom == (axis, positive)) positive = !positive;
    _unalign();
    if (axis == 1) {
      yaw = 0;
      pitch = positive ? -maxPitch : maxPitch;
    } else {
      final side = [0.0, 0.0, 0.0]..[axis] = positive ? 1.0 : -1.0;
      _lookAlong(vScale(side, -1));
    }
  }

  /// The axis side the camera looks from — (axis, positive side) — when it
  /// looks straight along an axis; null between them.
  (int, bool)? get viewFrom {
    final f = forward;
    for (var axis = 0; axis < 3; axis++) {
      if (f[axis].abs() >= _alignedCos) return (axis, f[axis] < 0);
    }
    return null;
  }

  /// How close to an axis [forward] must be to count as looking along it
  /// (the vertical views stop half a degree short, at [maxPitch]).
  static final _alignedCos = math.cos(0.75 * math.pi / 180);

  /// The name of the view the camera looks from — the engine's front is +Z
  /// (the default camera looks along −Z) — or null between them.
  String? get viewName => switch (viewFrom) {
    (0, true) => 'Right',
    (0, false) => 'Left',
    (1, true) => 'Top',
    (1, false) => 'Bottom',
    (2, true) => 'Front',
    (2, false) => 'Back',
    _ => null,
  };

  Map<String, Object?> toJson() => {
    'pivot': pivot,
    'distance': distance,
    'yaw': yaw,
    'pitch': pitch,
    'orthographic': orthographic,
    'fovY': fovY,
    'near': near,
    'far': far,
    if (manualExposure != null) 'exposure': manualExposure,
    if (alignedWorld != null) 'alignedWorld': alignedWorld,
  };

  static EditorCamera? fromJson(Object? json) {
    if (json is! Map) return null;
    List<double>? vec(Object? v, int n) => v is List && v.length == n && v.every((x) => x is num && x.isFinite)
        ? [for (final x in v) (x as num).toDouble()]
        : null;
    double num_(String k, double fallback) {
      final v = json[k];
      return v is num && v.isFinite ? v.toDouble() : fallback;
    }

    final c = EditorCamera(
      pivot: vec(json['pivot'], 3),
      distance: math.max(minDistance, num_('distance', 10)),
      yaw: num_('yaw', -0.6),
      pitch: num_('pitch', -0.5).clamp(-maxPitch, maxPitch),
      orthographic: json['orthographic'] == true,
      fovY: num_('fovY', 60 * math.pi / 180).clamp(1 * math.pi / 180, 179 * math.pi / 180),
      near: num_('near', 0.1),
      far: num_('far', 1000),
      manualExposure: json['exposure'] is num && (json['exposure'] as num).isFinite
          ? (json['exposure'] as num).toDouble()
          : null,
      alignedWorld: vec(json['alignedWorld'], 16),
    );
    if (c.near <= 0 || c.far <= c.near) {
      c.near = 0.1;
      c.far = 1000;
    }
    return c;
  }

  EditorCamera copy() => EditorCamera.fromJson(toJson())!;
}

/// A camera at a viewport size: the pixel ↔ world conversions.
final class ViewProjection {
  ViewProjection(this.camera, this.width, this.height)
    : aspect = width / math.max(height, 1e-6),
      view = camera.view,
      world = camera.world,
      proj = camera.projection(width / math.max(height, 1e-6));

  final EditorCamera camera;

  /// Physical pixels.
  final double width, height;
  final double aspect;
  final List<double> view, world, proj;

  late final List<double> viewProj = matMul(proj, view);

  /// Normalised device coordinates (−1..1, +Y up) of a pixel (origin
  /// top-left, +Y down).
  List<double> ndcOf(double px, double py) => [px / width * 2 - 1, 1 - py / height * 2];

  /// The pixel of a world point, or null behind the camera. The third
  /// value is the NDC depth (for ordering).
  List<double>? project(Vec3 p) {
    final m = viewProj;
    final x = m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12];
    final y = m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13];
    final z = m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14];
    final w = m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15];
    if (w <= 1e-9) return null;
    return [(x / w + 1) / 2 * width, (1 - y / w) / 2 * height, z / w];
  }

  /// The ray through a pixel, in world space (the same construction as
  /// `fynkel_authoring::View::ray`).
  Ray rayAt(double px, double py) {
    final ndc = ndcOf(px, py);
    final tanHalf = math.tan(camera.fovY / 2);
    final axes = matAxes(world);
    final eye = matTranslation(world);
    if (camera.orthographic) {
      final h = camera.orthoHeight / 2;
      final origin = vAdd(vAdd(eye, vScale(axes[0], ndc[0] * h * aspect)), vScale(axes[1], ndc[1] * h));
      return Ray(origin, vNormalize(vScale(axes[2], -1)));
    }
    final dirCam = [ndc[0] * tanHalf * aspect, ndc[1] * tanHalf, -1.0];
    return Ray(eye, vNormalize(transformDirection(world, dirCam)));
  }

  /// Metres per pixel at the depth of [p] (a handle of constant screen size).
  double metresPerPixel(Vec3 p) {
    if (camera.orthographic) return camera.orthoHeight / height;
    final d = vDot(vSub(p, matTranslation(world)), vScale(matAxes(world)[2], -1));
    return 2 * math.max(d, camera.near) * math.tan(camera.fovY / 2) / height;
  }
}

/// The corners of an axis-aligned box, for framing.
List<Vec3> boxCorners(Vec3 min, Vec3 max) => [
  for (final x in [min[0], max[0]])
    for (final y in [min[1], max[1]])
      for (final z in [min[2], max[2]]) [x, y, z],
];

/// Rounds [v] to the nearest multiple of [step] (a positive step; 0 leaves it).
double snapTo(double v, double step) => step <= 0 ? v : (v / step).roundToDouble() * step;
