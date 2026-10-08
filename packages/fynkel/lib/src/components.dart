part of 'world.dart';

/// Something an object has: a [Transform], [Camera], [Light] or
/// [MeshRenderer]. Components are values. Reading one from an object gives
/// a copy, and changing the object means giving it a new value.
sealed class Component {
  const Component();
}

/// Where an object is, which way it faces, and how big it is, relative to
/// its [GameObject.parent] (or the world, for a root). [GameObject.worldTransform]
/// gives it in the world.
final class Transform extends Component {
  /// [lookAt] turns the object so it looks at that point from [position];
  /// [forward] turns it to look along that direction. Give at most one of
  /// [rotation], [lookAt] and [forward].
  factory Transform({Vec3 position = Vec3.zero, Quat? rotation, Vec3 scale = Vec3.one, Vec3? lookAt, Vec3? forward}) {
    if ((rotation != null ? 1 : 0) + (lookAt != null ? 1 : 0) + (forward != null ? 1 : 0) > 1) {
      throw ArgumentError('give at most one of rotation, lookAt and forward');
    }
    final r = switch ((lookAt, forward)) {
      (final Vec3 target, _) => Quat.lookRotation(target - position),
      (_, final Vec3 direction) => Quat.lookRotation(direction),
      _ => rotation ?? Quat.identity,
    };
    return Transform._(position, r, scale);
  }

  const Transform._(this.position, this.rotation, this.scale);

  final Vec3 position;
  final Quat rotation;
  final Vec3 scale;

  /// The way the object looks: its -Z, turned by [rotation].
  Vec3 get forward => rotation.rotate(Vec3.forward);

  /// Turned [angle] radians around [axis], where [axis] is in the object's
  /// own frame (so `Vec3.up` is the object's up, as in Unity's `Rotate`).
  Transform rotated(Vec3 axis, double angle) =>
      Transform._(position, (rotation * Quat.axisAngle(axis, angle)).normalized, scale);

  /// Moved by [offset], in the parent's space (the world's for a root).
  Transform translated(Vec3 offset) => Transform._(position + offset, rotation, scale);

  Transform copyWith({Vec3? position, Quat? rotation, Vec3? scale}) =>
      Transform._(position ?? this.position, rotation ?? this.rotation, scale ?? this.scale);

  @override
  bool operator ==(Object other) =>
      other is Transform && other.position == position && other.rotation == rotation && other.scale == scale;

  @override
  int get hashCode => Object.hash(position, rotation, scale);

  @override
  String toString() => 'Transform(position: $position, rotation: $rotation, scale: $scale)';
}

/// Makes the object a camera. The view is drawn from the camera with the
/// lowest id that also has a [Transform].
final class Camera extends Component {
  const Camera({this.fovY = 60, this.near = 0.1, this.far = 1000, this.clearColor = const Color.rgb(0.1, 0.1, 0.12)});

  /// Vertical field of view in degrees, 1–179.
  final double fovY;
  final double near;
  final double far;

  /// What the view shows where nothing is drawn.
  final Color clearColor;

  @override
  bool operator ==(Object other) =>
      other is Camera && other.fovY == fovY && other.near == near && other.far == far && other.clearColor == clearColor;

  @override
  int get hashCode => Object.hash(fovY, near, far, clearColor);

  @override
  String toString() => 'Camera(fovY: $fovY, near: $near, far: $far, clearColor: $clearColor)';
}

enum LightKind {
  /// Light from far away, like the sun, shining along the object's forward
  /// (its [Transform.forward]; without a transform it doesn't shine). Only
  /// the one with the lowest id is used.
  sun,

  /// Light from every direction, adding to every surface. All ambient
  /// lights add up.
  ambient,
}

final class Light extends Component {
  /// Shines along the object's [Transform.forward]: spawn it with
  /// `Transform(forward: direction)`.
  const Light.sun({this.color = Color.white, this.intensity = 1}) : kind = LightKind.sun;

  const Light.ambient({this.color = Color.white, this.intensity = 1}) : kind = LightKind.ambient;

  final LightKind kind;
  final Color color;
  final double intensity;

  @override
  bool operator ==(Object other) =>
      other is Light && other.kind == kind && other.color == color && other.intensity == intensity;

  @override
  int get hashCode => Object.hash(kind, color, intensity);

  @override
  String toString() => 'Light.${kind.name}(color: $color, intensity: $intensity)';
}

/// Draws [mesh] with [material] at the object's [Transform]. Without both,
/// nothing is drawn.
final class MeshRenderer extends Component {
  const MeshRenderer([this.mesh, this.material]);

  final Mesh? mesh;
  final Material? material;

  @override
  bool operator ==(Object other) =>
      other is MeshRenderer && identical(other.mesh, mesh) && identical(other.material, material);

  @override
  int get hashCode => Object.hash(mesh, material);

  @override
  String toString() => 'MeshRenderer($mesh, $material)';
}
