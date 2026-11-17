part of 'world.dart';

/// Something on a [GameObject]: a [Camera], [Light] or [MeshRenderer]
/// (the engine's), or a [Behaviour] (the game's own code). An object has
/// any number of behaviours and at most one of each of the engine's. A
/// component belongs to the one object it was given to, for its life: put
/// it on with `GameObject(components: [...])` or
/// [GameObject.addComponent], find it with [GameObject.get], and take it
/// off with [GameObject.removeComponent].
///
/// Its [transform] is its object's: every object has one, so it isn't a
/// component here.
sealed class Component {
  Component._();

  GameObject? _owner;
  bool _removed = false;

  // Taken off with [GameObject.removeComponent], not gone with its object:
  // [GameObject.get] no longer finds it.
  bool _takenOff = false;

  // Whether the systems were told it came ([WorldSystem.componentAdded]),
  // so they're told when it goes: not a model part's, read from the engine.
  bool _announced = false;

  /// Whether it's on (Unity's `enabled`). Off, a [Camera] draws nothing, a
  /// [Light] lights nothing (and isn't one of a frame's 64 lamps), a
  /// [MeshRenderer] draws nothing, and a [Behaviour] gets none of its
  /// object's frames or pointers; its object and its other components go
  /// on as they were. It keeps what it was given, and changes made while
  /// it's off show when it's on again.
  bool get enabled => _enabledFlag;
  set enabled(bool value) {
    if (value == _enabledFlag) return;
    _enabledFlag = value;
    if (_live case final object?) _enabledChanged(object);
  }

  bool _enabledFlag = true;

  /// Tells the engine or the world that [enabled] changed on live [object]:
  /// an engine component is put in the engine, or taken out.
  void _enabledChanged(GameObject object) => _enabledFlag ? _put(object) : _takeOut(object);

  // The one way an engine component comes into a world and goes (DC5):
  // each writes itself, and keeps what its system holds of it, so neither
  // the object nor the world names the kinds.

  /// Puts it, on, in the engine on live [object] as it is now, or again: an
  /// engine component writes itself. Throws what the engine refuses.
  void _put(GameObject object) {}

  /// Takes it, put and on, out of the engine on live [object], and out of
  /// what its system holds.
  void _takeOut(GameObject object) {}

  /// Forgets what it told the world when the engine let go of it with its
  /// object: destroyed, or its add refused.
  void _forget(GameObject object) {}

  /// The object it's on. Throws a [StateError] before it's given to one.
  GameObject get gameObject => _owner ?? (throw StateError('$runtimeType is on no object yet'));

  /// Whether it was taken off its object ([GameObject.removeComponent]) or
  /// its object was destroyed. It can still be read, but not used; one
  /// that went with its object is still found by the object's
  /// [GameObject.get].
  bool get isRemoved => _removed;

  /// Its object's [GameObject.transform].
  Transform get transform => gameObject.transform;
  set transform(Transform value) => gameObject.transform = value;

  /// Its object's component of type [T]: see [GameObject.get].
  T? get<T extends Object>() => gameObject.get<T>();

  /// Its object's components of type [T]: see [GameObject.getAll].
  List<T> getAll<T extends Object>() => gameObject.getAll<T>();

  /// Whether its object has a component of type [T].
  bool has<T extends Object>() => gameObject.has<T>();

  /// The nearest object above its object that is a [T], or null: see
  /// [GameObject.ancestor]. `ancestor<ArenaGame>()` finds the game a
  /// behaviour's object is in.
  T? ancestor<T extends GameObject>() => gameObject.ancestor<T>();

  /// The first component of type [T] on its object or the nearest one
  /// above: see [GameObject.getInParent].
  T? getInParent<T extends Object>({bool includeInactive = false}) =>
      gameObject.getInParent<T>(includeInactive: includeInactive);

  /// The first component of type [T] on its object or below it: see
  /// [GameObject.getInChildren].
  T? getInChildren<T extends Object>({bool includeInactive = false}) =>
      gameObject.getInChildren<T>(includeInactive: includeInactive);

  /// Every component of type [T] on its object and below it: see
  /// [GameObject.getAllInChildren].
  List<T> getAllInChildren<T extends Object>({bool includeInactive = false}) =>
      gameObject.getAllInChildren<T>(includeInactive: includeInactive);

  /// The live object it's on, for writing a change to the engine; null
  /// while there's nothing to write to (not added yet, or the part of a
  /// model being read).
  GameObject? get _live => switch (_owner) {
    final o? when o._world != null && !_removed => o,
    _ => null,
  };

  /// Throws a [StateError] once it's removed: a change would do nothing.
  void _checkChangeable() {
    if (!_removed) return;
    final owner = _owner;
    if (owner == null) return;
    throw StateError(
      owner._destroyed
          ? '$owner was destroyed, so its $runtimeType can\'t be changed'
          : '$runtimeType was removed from $owner, so it can\'t be changed',
    );
  }
}

/// Where an object is, which way it faces, and how big it is, relative to
/// its [GameObject.parent] (or the world, for a root). [GameObject.worldTransform]
/// gives it in the world.
final class Transform {
  /// Its rotation is a [Quat], as Unity's is a Quaternion: to face a way,
  /// `rotation: Quat.lookRotation(direction)`; by angles,
  /// `rotation: Quat.euler(pitch, yaw, roll)`. To face a point in the
  /// world once it's added, [GameObject.lookAt].
  const Transform({this.position = Vec3.zero, this.rotation = Quat.identity, this.scale = Vec3.one});

  const Transform._(this.position, this.rotation, this.scale);

  /// At [eye], looking at [target] with its up as close to [up] as it can
  /// be: a camera's place, `Transform.lookAt(Vec3(0, 6, 8), Vec3.zero)`.
  /// Throws an [ArgumentError] if they're the same point.
  Transform.lookAt(Vec3 eye, Vec3 target, {Vec3 up = Vec3.up, this.scale = Vec3.one})
    : position = eye,
      rotation = Quat.lookRotation(target - eye, up: up);

  /// Where it is, in metres, from its parent's origin and along its
  /// parent's axes (the world's for a root): Unity's `localPosition`.
  /// NaN or infinity is refused with an ArgumentError when it's set on an
  /// object.
  final Vec3 position;

  /// Which way it's turned, relative to its parent (Unity's
  /// `localRotation`). A unit quaternion: one that isn't (within 0.001)
  /// is refused with an ArgumentError when it's set on an object; use
  /// [Quat.normalized].
  final Quat rotation;

  /// How big it is along its own axes, times its parent's: 1 is its size
  /// as made (Unity's `localScale`). NaN or infinity is refused with an
  /// ArgumentError when it's set on an object.
  final Vec3 scale;

  /// Turned [angle] radians around [axis], where [axis] is in the object's
  /// own frame (so `Vec3.up` is the object's up, as in Unity's `Rotate`).
  Transform rotated(Vec3 axis, double angle) =>
      Transform._(position, (rotation * Quat.axisAngle(axis, angle)).normalized, scale);

  /// Moved by [offset], in the parent's space (the world's for a root).
  Transform translated(Vec3 offset) => Transform._(position + offset, rotation, scale);

  /// This transform with the parts given changed.
  Transform copyWith({Vec3? position, Quat? rotation, Vec3? scale}) =>
      Transform._(position ?? this.position, rotation ?? this.rotation, scale ?? this.scale);

  /// [local], a child's, in the space this transform is in, as Unity
  /// works it out: the position exactly, the rotation this one's times the
  /// child's, and the scale this one's times the child's (Unity's "lossy"
  /// scale, approximate under a rotated child of an uneven scale).
  Transform _then(Transform local) => Transform._(
    position + rotation.rotate(local.position.scaledBy(scale)),
    (rotation * local.rotation).normalized,
    scale.scaledBy(local.scale),
  );

  /// The transform that, under this one, is [world]: [_then]'s other way.
  /// A side of this scale that is 0 keeps [keepScale]'s.
  Transform _under(Transform world, Vec3 keepScale) {
    final p = rotation.inverse.rotate(world.position - position);
    double div(double a, double b, double keep) => b == 0 ? keep : a / b;
    return Transform._(
      Vec3(div(p.x, scale.x, 0), div(p.y, scale.y, 0), div(p.z, scale.z, 0)),
      (rotation.inverse * world.rotation).normalized,
      Vec3(
        div(world.scale.x, scale.x, keepScale.x),
        div(world.scale.y, scale.y, keepScale.y),
        div(world.scale.z, scale.z, keepScale.z),
      ),
    );
  }

  @override
  bool operator ==(Object other) =>
      other is Transform && other.position == position && other.rotation == rotation && other.scale == scale;

  @override
  int get hashCode => Object.hash(position, rotation, scale);

  @override
  String toString() => 'Transform(position: $position, rotation: $rotation, scale: $scale)';
}

/// The space a move or a turn is in: the object's own axes ([self], as
/// Unity's `Space.Self`, what [GameObject.translate] and
/// [GameObject.rotate] use unless told), or the world's.
enum Space {
  /// The object's own axes: forward is the way it faces.
  self,

  /// The world's axes.
  world,
}

/// For views: the id of [camera]'s object while it's a live object of
/// [world], else -1, which names no object.
int cameraObjectId(Camera camera, World world) {
  final object = camera._owner;
  if (object == null || camera.isRemoved || !identical(object._world, world) || !object.isAlive) return -1;
  return object._id;
}

/// Light brighter than white glowing into what's around it: a lamp's
/// bulb, a glowing sword, the sun on metal ([Camera.bloom]). Drawn where
/// the world's [Quality.bloom] is on.
final class Bloom {
  /// Bloom of [strength] from [threshold]: by default, about as much glow
  /// as light, from what's brighter than white.
  const Bloom({this.strength = 1, this.threshold = 1});

  /// How much of the light glows, 0 to 10: 0, none; 1, about as much as
  /// it gives off itself, spread wide.
  final double strength;

  /// How bright light must be to glow, 0 to 100: at 1, only what's
  /// brighter than white (a lit white wall stays as it is), fading in over
  /// half as much again. A material glows with [Material.emissive] times an
  /// [Material.emissiveIntensity] above 1.
  final double threshold;

  /// A copy with the values given changed.
  Bloom copyWith({double? strength, double? threshold}) =>
      Bloom(strength: strength ?? this.strength, threshold: threshold ?? this.threshold);

  @override
  bool operator ==(Object other) => other is Bloom && other.strength == strength && other.threshold == threshold;

  @override
  int get hashCode => Object.hash(strength, threshold);

  @override
  String toString() => 'Bloom(strength: ${strength.toDouble()}, threshold: ${threshold.toDouble()})';
}

/// Surfaces darkening where they meet, in corners and creases and under
/// things on the ground: a soft hint of contact ([Camera.ambientOcclusion]).
/// Only the ambient and sky light darken; lights and their shadows stay as
/// they are. Drawn where the world's [Quality.ambientOcclusion] is on.
final class AmbientOcclusion {
  /// Ambient occlusion of [intensity] over [radius] metres: by default, as
  /// dark as corners are closed in, over half a metre.
  const AmbientOcclusion({this.intensity = 1, this.radius = 0.5});

  /// How dark, 0 to 4: 0, not at all; 1, as dark as the corner is closed
  /// in.
  final double intensity;

  /// How far, in metres, a surface darkens one near it, 0 to 10.
  final double radius;

  /// A copy with the values given changed.
  AmbientOcclusion copyWith({double? intensity, double? radius}) =>
      AmbientOcclusion(intensity: intensity ?? this.intensity, radius: radius ?? this.radius);

  @override
  bool operator ==(Object other) => other is AmbientOcclusion && other.intensity == intensity && other.radius == radius;

  @override
  int get hashCode => Object.hash(intensity, radius);

  @override
  String toString() => 'AmbientOcclusion(intensity: ${intensity.toDouble()}, radius: ${radius.toDouble()})';
}

/// Makes the object a camera. A view is drawn from the shown object
/// with a camera and the lowest id ([World.camera]), unless it has a
/// camera of its own ([WorldView.camera]): a minimap, the other half of a
/// split screen. Change it in place: `get<Camera>()!.fovY = 40.deg`.
///
/// A value out of its range, or NaN, throws an [ArgumentError] from the
/// constructor or the setter, naming it, and a setter leaves the old value
/// in place.
final class Camera extends Component {
  /// A perspective camera with a 60° [fovY], [near] 0.1 m, [far] 1,000 m,
  /// a dark grey [clearColor], drawing every layer, without [bloom] or
  /// [ambientOcclusion]. Pass [orthographic] for a parallel one,
  /// [orthographicSize] 5 unless given.
  Camera({
    this._fovY = 60 * (math.pi / 180),
    this._near = 0.1,
    this._far = 1000,
    this._clearColor = const Color.rgb(0.1, 0.1, 0.12),
    this._orthographic = false,
    this._orthographicSize = 5,
    this._cullingMask = LayerMask.all,
    this._showSky = true,
    this._bloom,
    this._ambientOcclusion,
    this._pixels,
    List<ScreenEffect> effects = const [],
  }) : super._() {
    if (_refused() case final error?) throw error;
    _useEffects(effects);
  }

  double _fovY, _near, _far, _orthographicSize;
  int? _pixels;
  List<ScreenEffect> _effects = const [];

  /// Whether the engine was given effects, so taking them away is sent.
  bool _effectsSent = false;
  Color _clearColor;
  bool _orthographic, _showSky;
  LayerMask _cullingMask;
  Bloom? _bloom;
  AmbientOcclusion? _ambientOcclusion;

  /// Vertical field of view in radians, 1° to 179°: 60.deg unless set.
  double get fovY => _fovY;
  set fovY(double value) {
    _checkChangeable();
    final old = _fovY;
    _fovY = value;
    if (_write() case final failed?) {
      _fovY = old;
      failed.raise();
    }
  }

  /// How near to the camera things are drawn, in metres (Unity's
  /// `nearClipPlane`): 0.1 unless set, 0.0001 to 1,000,000. Keep it below
  /// [far]: a camera whose isn't draws nothing, so a view drawing from it
  /// throws an [ArgumentError] from its frame, naming it. Checked only
  /// then, so setting one and then the other is fine.
  double get near => _near;
  set near(double value) {
    _checkChangeable();
    final old = _near;
    _near = value;
    if (_write() case final failed?) {
      _near = old;
      failed.raise();
    }
  }

  /// How far from the camera things are drawn, in metres (Unity's
  /// `farClipPlane`): 1,000 unless set, 0.001 to 1,000,000,000.
  double get far => _far;
  set far(double value) {
    _checkChangeable();
    final old = _far;
    _far = value;
    if (_write() case final failed?) {
      _far = old;
      failed.raise();
    }
  }

  /// What the view shows where nothing is drawn (Unity's
  /// `backgroundColor`).
  Color get clearColor => _clearColor;
  set clearColor(Color value) {
    _checkChangeable();
    final old = _clearColor;
    _clearColor = value;
    if (_write() case final failed?) {
      _clearColor = old;
      failed.raise();
    }
  }

  /// Parallel projection: things keep their size however far away they
  /// are, and [fovY] is unused. For a map, a board game or a side view.
  bool get orthographic => _orthographic;
  set orthographic(bool value) {
    _checkChangeable();
    final old = _orthographic;
    _orthographic = value;
    if (_write() case final failed?) {
      _orthographic = old;
      failed.raise();
    }
  }

  /// Half the view's height, in metres, when [orthographic] (Unity's
  /// `orthographicSize`): 5 shows 10 m from top to bottom.
  double get orthographicSize => _orthographicSize;
  set orthographicSize(double value) {
    _checkChangeable();
    final old = _orthographicSize;
    _orthographicSize = value;
    if (_write() case final failed?) {
      _orthographicSize = old;
      failed.raise();
    }
  }

  /// The layers it draws (Unity's `cullingMask`): objects on the others
  /// ([GameObject.layer]) aren't drawn, nor tapped through it. Lights light
  /// every layer.
  LayerMask get cullingMask => _cullingMask;
  set cullingMask(LayerMask value) {
    _checkChangeable();
    final old = _cullingMask;
    _cullingMask = value;
    if (_write() case final failed?) {
      _cullingMask = old;
      failed.raise();
    }
  }

  /// Whether it shows the world's [World.sky] behind everything: true
  /// unless set. Without it, [clearColor] shows; the sky still lights
  /// the scene.
  bool get showSky => _showSky;
  set showSky(bool value) {
    _checkChangeable();
    final old = _showSky;
    _showSky = value;
    if (_write() case final failed?) {
      _showSky = old;
      failed.raise();
    }
  }

  /// Whether light brighter than white glows into what's around it, and
  /// how: null unless set, for none; `const Bloom()` for about as much
  /// glow as light. Drawn where the world's [Quality.bloom] is on (the
  /// medium and high levels); a debug build says once if it isn't.
  Bloom? get bloom => _bloom;
  set bloom(Bloom? value) {
    _checkChangeable();
    final old = _bloom;
    _bloom = value;
    if (_write() case final failed?) {
      _bloom = old;
      failed.raise();
    }
  }

  /// Whether surfaces darken where they meet, and how: null unless set,
  /// for not; `const AmbientOcclusion()` for corners as dark as they're
  /// closed in. Drawn where the world's [Quality.ambientOcclusion] is on
  /// (the high level); a debug build says once if it isn't.
  AmbientOcclusion? get ambientOcclusion => _ambientOcclusion;
  set ambientOcclusion(AmbientOcclusion? value) {
    _checkChangeable();
    final old = _ambientOcclusion;
    _ambientOcclusion = value;
    if (_write() case final failed?) {
      _ambientOcclusion = old;
      failed.raise();
    }
  }

  /// The pixel look: the game drawn this many pixels high (and as wide as
  /// the view's shape makes it), then enlarged to the view with hard
  /// edges, as `pixels = 240`. The camera is put on the nearest whole
  /// pixel, so what's still doesn't shimmer as it moves: exactly when
  /// [orthographic]; for a perspective camera, for what's where it looks
  /// at the ground. Null (the default) for the view's own size; 1 to
  /// 16,384, and never more than the view's height.
  int? get pixels => _pixels;
  set pixels(int? value) {
    _checkChangeable();
    if (value != null && (value < 1 || value > 16384)) {
      throw RangeError.range(value, 1, 16384, 'pixels');
    }
    final old = _pixels;
    _pixels = value;
    if (_write() case final failed?) {
      _pixels = old;
      failed.raise();
    }
  }

  /// What runs over its picture once it's drawn, in order, each over what
  /// the one before made: `[ScreenEffect(oldTv)]`. Empty (the default) for
  /// the picture as it is. They run at every quality level; a frame with
  /// none pays nothing for them. Set a new list to change them.
  List<ScreenEffect> get effects => _effects;
  set effects(List<ScreenEffect> value) {
    _checkChangeable();
    final old = _effects;
    _useEffects(value);
    if (_writeEffects() case final failed?) {
      _useEffects(old);
      failed.raise();
    }
  }

  void _useEffects(List<ScreenEffect> value) {
    for (final effect in _effects) {
      effect._cameras.removeWhere((c) => identical(c.target, this));
    }
    for (final effect in value) {
      if (effect.shader.isDisposed) throw StateError('${effect.shader} was disposed');
    }
    _effects = List.unmodifiable(value);
    for (final effect in _effects) {
      _addWeak(effect._cameras, this);
    }
  }

  /// Sends its [effects] to the engine, as [_write] does the rest.
  _Failure? _writeEffects() {
    if (!_enabledFlag) return null;
    try {
      _live?._writeCameraEffects(this);
      return null;
    } catch (error, stack) {
      return _Failure(error, stack);
    }
  }

  /// The ray from this camera through [point] in a view showing it: from
  /// the point on the near plane, away from the camera. Cast it with
  /// [World.raycast] to find what's drawn there (Unity's
  /// `ScreenPointToRay`).
  ///
  /// Uses its object's [GameObject.worldTransform] (its scale is ignored,
  /// as the view ignores it). Throws a [StateError] when it's on no object.
  Ray screenPointToRay(ViewPoint point) {
    if (!(point.width > 0 && point.height > 0)) {
      throw ArgumentError('screenPointToRay needs a point in a view with an area; got $point', 'point');
    }
    final c = this;
    final t = gameObject.worldTransform;
    // Across the view from -1 to 1, up positive, so the view's y flips.
    final across = 2 * point.x / point.width - 1;
    final up = 1 - 2 * point.y / point.height;
    final aspect = point.width / point.height;
    if (c.orthographic) {
      // Straight ahead, from the point on the near plane.
      final from = Vec3(across * c.orthographicSize * aspect, up * c.orthographicSize, -c.near);
      return Ray(t.position + t.rotation.rotate(from), t.rotation.rotate(Vec3.forward));
    }
    final tanHalf = math.tan(c.fovY / 2);
    // Through the point on the plane 1 in front of the camera.
    final through = t.rotation.rotate(Vec3(across * tanHalf * aspect, up * tanHalf, -1));
    return Ray(t.position + through * c.near, through);
  }

  /// Where the ray through [point] meets [plane]: the spot on the ground
  /// under a finger, say. Null when the ray runs along the plane or away
  /// from it.
  Vec3? screenPointToPlane(ViewPoint point, Plane plane) {
    final ray = screenPointToRay(point);
    final distance = plane.raycast(ray);
    return distance == null ? null : ray.at(distance);
  }

  /// Where [point], in the world, is in a view showing this camera
  /// (Unity's `WorldToScreenPoint`), in the view's logical pixels from its
  /// top left: in [width] × [height], by default the size [World.input]
  /// was last given. Null when it's behind a perspective camera; it can be
  /// outside the view.
  ViewPoint? worldToScreenPoint(Vec3 point, {double? width, double? height}) {
    final input = gameObject.world.input;
    final w = width ?? input.width, h = height ?? input.height;
    if (!(w > 0 && h > 0)) {
      throw ArgumentError(
        'worldToScreenPoint needs a view with an area, and the world\'s input has ${w}x$h: give the view\'s size '
        '(width: and height:), or in a test, give the input one with world.input.simulate.resize(width, height)',
      );
    }
    final c = this;
    final t = gameObject.worldTransform;
    final p = t.rotation.inverse.rotate(point - t.position);
    final aspect = w / h;
    double across, up;
    if (c.orthographic) {
      across = p.x / (c.orthographicSize * aspect);
      up = p.y / c.orthographicSize;
    } else {
      final ahead = -p.z;
      if (!(ahead > 0)) return null;
      final tanHalf = math.tan(c.fovY / 2);
      across = p.x / (ahead * tanHalf * aspect);
      up = p.y / (ahead * tanHalf);
    }
    return ViewPoint((across + 1) / 2 * w, (1 - up) / 2 * h, width: w, height: h);
  }

  @override
  void _put(GameObject object) => object._writeCamera(this);

  @override
  void _takeOut(GameObject object) {
    final w = object.world.pointer;
    // Its effects go with it, so their shaders can be let go.
    if (_effectsSent) {
      check(fy_camera_effects_set(w, object.id, nullptr, 0));
      _effectsSent = false;
    }
    check(fy_camera_remove(w, object.id));
  }

  @override
  void _forget(GameObject object) => _effectsSent = false;

  /// Writes it to the engine; what it or the engine refuses comes back,
  /// for the setter to undo its change and throw. Off, it's only kept
  /// here.
  _Failure? _write() {
    if (_refused() case final error?) return _Failure(error, StackTrace.current);
    if (!_enabledFlag) return null;
    try {
      _live?._writeCamera(this);
      return null;
    } catch (error, stack) {
      return _Failure(error, stack);
    }
  }

  /// Why it can't be as it is, if it can't: the engine's ranges.
  ArgumentError? _refused() {
    final on = _owner;
    if (!(_fovY >= _degree - _slack && _fovY <= 179 * _degree + _slack)) {
      return _rangeError('Camera.fovY', _fovY, 1, 179, radians: true, owner: on);
    }
    if (!(_near >= 1e-4 && _near <= 1e6)) {
      return _rangeError('Camera.near', _near, 1e-4, 1e6, unit: ' metres', owner: on);
    }
    if (!(_far >= 1e-3 && _far <= 1e9)) return _rangeError('Camera.far', _far, 1e-3, 1e9, unit: ' metres', owner: on);
    if (!(_orthographicSize >= 1e-4 && _orthographicSize <= 1e6)) {
      return _rangeError('Camera.orthographicSize', _orthographicSize, 1e-4, 1e6, unit: ' metres', owner: on);
    }
    if (_notFinite(_clearColor)) return _colorError('Camera.clearColor', _clearColor, on);
    if (_bloom case final b?) {
      if (!(b.strength >= 0 && b.strength <= 10)) return _rangeError('Bloom.strength', b.strength, 0, 10, owner: on);
      if (!(b.threshold >= 0 && b.threshold <= 100)) {
        return _rangeError('Bloom.threshold', b.threshold, 0, 100, owner: on);
      }
    }
    if (_ambientOcclusion case final ao?) {
      if (!(ao.intensity >= 0 && ao.intensity <= 4)) {
        return _rangeError('AmbientOcclusion.intensity', ao.intensity, 0, 4, owner: on);
      }
      if (!(ao.radius >= 0 && ao.radius <= 10)) {
        return _rangeError('AmbientOcclusion.radius', ao.radius, 0, 10, unit: ' metres', owner: on);
      }
    }
    return null;
  }

  @override
  String toString() =>
      'Camera(${orthographic ? 'orthographic, orthographicSize: $orthographicSize' : 'fovY: $fovY'}, near: $near, far: $far, '
      'clearColor: $clearColor${cullingMask == LayerMask.all ? '' : ', cullingMask: $cullingMask'}'
      '${showSky ? '' : ', showSky: false'}'
      '${bloom == null ? '' : ', bloom: $bloom'}'
      '${ambientOcclusion == null ? '' : ', ambientOcclusion: $ambientOcclusion'}'
      '${pixels == null ? '' : ', pixels: $pixels'}'
      '${effects.isEmpty ? '' : ', effects: $effects'})';
}

/// What kind of light a [Light] is.
enum LightKind {
  /// Light from far away, like the sun, shining along the object's forward
  /// (its [GameObject.forward]): Unity's `LightType.Directional`. It
  /// lights everything. Up to four light a frame, the first ones made.
  sun,

  /// Light from every direction, adding to every surface, as Unity's
  /// `RenderSettings.ambientLight`. All ambient lights add up.
  ambient,

  /// A lamp: light from the object's position in every direction, falling
  /// off with distance and gone at its [Light.range]. Unity's
  /// `LightType.Point`.
  point,

  /// A spotlight: a lamp that shines only in a cone around the object's
  /// forward, [Light.angle] across. Unity's `LightType.Spot`.
  spot,
}

/// What a light counts as against a frame's caps.
enum _LightCount {
  none,
  sun,
  lamp;

  /// What a light of [kind] counts as.
  static _LightCount of(LightKind kind) => switch (kind) {
    LightKind.sun => sun,
    LightKind.point || LightKind.spot => lamp,
    LightKind.ambient => none,
  };
}

/// Lights the scene. Change it in place: `get<Light>()!.intensity = 2`.
final class Light extends Component {
  /// Shines along the object's [GameObject.forward]. Given a [direction],
  /// it turns the object it's put on to shine that way:
  /// `GameObject(components: [Light.sun(direction: Vec3(-0.4, -1, -0.3))])`.
  /// What it shines on casts shadows unless [shadows] is false (see
  /// [Light.shadows]), as a new scene's sun does in Unity.
  factory Light.sun({Vec3? direction, Color color = Color.white, double intensity = 1, bool shadows = true}) {
    final facing = direction == null ? null : Quat.lookRotation(direction);
    return Light._(LightKind.sun, color, intensity, 10, 30 * math.pi / 180, 0.2, shadows).._facing = facing;
  }

  // The way a sun given a direction turns the object it's put on, until
  // it's put on one.
  Quat? _facing;

  /// Light from every direction: see [LightKind.ambient]. A dim one, say
  /// `Light.ambient(intensity: 0.25)`, keeps the sides the sun misses from
  /// going black.
  Light.ambient({Color color = Color.white, double intensity = 1}) : this._(LightKind.ambient, color, intensity);

  /// A lamp at the object's position, reaching [range] metres. Light falls
  /// off with the square of the distance: a lamp of [intensity] 1 lights a
  /// surface 1 m away as a sun of 1 does, and a quarter as much at 2 m. A
  /// room lamp 2–3 m from what it lights wants 5 to 20. It casts no
  /// shadows yet: a spotlight does.
  Light.point({Color color = Color.white, double intensity = 1, double range = 10})
    : this._(LightKind.point, color, intensity, range);

  /// A spotlight at the object's position, shining along its forward in a
  /// cone [angle] across (`40.deg`), reaching [range] metres. [softness]
  /// is how much of the cone fades, from its rim in: 0 for a hard edge, 1
  /// to fade all the way to the middle. Brightness as [Light.point]. With
  /// [shadows], what it shines on casts shadows (see [Light.shadows]).
  Light.spot({
    Color color = Color.white,
    double intensity = 1,
    double range = 10,
    double angle = 30 * math.pi / 180,
    double softness = 0.2,
    bool shadows = false,
  }) : this._(LightKind.spot, color, intensity, range, angle, softness, shadows);

  Light._(
    this._kind,
    this._color,
    this._intensity, [
    this._range = 10,
    this._angle = 30 * math.pi / 180,
    this._softness = 0.2,
    this._shadows = false,
  ]) : super._() {
    if (_refused() case final error?) throw error;
  }

  /// The widest spot light that casts shadows: its map is at most 160°
  /// across, a little wider than its cone, and outside the map a surface
  /// is lit as if nothing stood in the way.
  static const _widestShadowedSpot = 150 * math.pi / 180;

  LightKind _kind;
  // What the world counts it as ([World._countLight]): a sun, a lamp or
  // neither.
  _LightCount _counted = _LightCount.none;
  Color _color;
  double _intensity, _range, _angle, _softness;
  bool _shadows;

  /// What kind of light it is; it can change while it's on.
  LightKind get kind => _kind;
  set kind(LightKind value) {
    _checkChangeable();
    final old = _kind;
    _kind = value;
    if (_write() case final failed?) {
      _kind = old;
      failed.raise();
    }
  }

  /// The light's colour: white unless set.
  Color get color => _color;
  set color(Color value) {
    _checkChangeable();
    final old = _color;
    _color = value;
    if (_write() case final failed?) {
      _color = old;
      failed.raise();
    }
  }

  /// How bright it is, 0 to 1,000,000; 1 unless set. A sun of 1 lights a
  /// white, rough surface that faces it to exactly its material's colour.
  ///
  /// This and the other values out of range, or NaN, throw an
  /// [ArgumentError] from the constructor or the setter, naming it, and a
  /// setter keeps the old value.
  double get intensity => _intensity;
  set intensity(double value) {
    _checkChangeable();
    final old = _intensity;
    _intensity = value;
    if (_write() case final failed?) {
      _intensity = old;
      failed.raise();
    }
  }

  /// A point or spot light's reach in metres, 0 to 1,000,000; 10 unless
  /// set. The light fades smoothly to nothing there. A parent's scale
  /// doesn't change it.
  double get range => _range;
  set range(double value) {
    _checkChangeable();
    final old = _range;
    _range = value;
    if (_write() case final failed?) {
      _range = old;
      failed.raise();
    }
  }

  /// A spot light's cone, edge to edge, in radians, 0 to 179° (`40.deg`);
  /// 30° unless set. One
  /// with [shadows] is at most 150°: wider is refused with an
  /// [ArgumentError].
  double get angle => _angle;
  set angle(double value) {
    _checkChangeable();
    final old = _angle;
    _angle = value;
    if (_write() case final failed?) {
      _angle = old;
      failed.raise();
    }
  }

  /// How much of a spot light's cone fades, from its rim in, 0 to 1; 0.2
  /// unless set.
  double get softness => _softness;
  set softness(double value) {
    _checkChangeable();
    final old = _softness;
    _softness = value;
    if (_write() case final failed?) {
      _softness = old;
      failed.raise();
    }
  }

  /// Whether what it shines on casts shadows: a sun's do unless it's made
  /// with `shadows: false`, other lights' only when set; and only
  /// where the world's [Quality.shadows] is on. A sun's reach
  /// [Quality.shadowDistance] from the camera and fade out there; only
  /// the first sun made with shadows casts them. A spot light's reach its
  /// [range]; [Quality.spotShadows] of them cast shadows, the ones nearest
  /// the camera, if they're at most
  /// 150° across ([angle]). A point light's
  /// shadows aren't drawn yet. A [MeshRenderer] can choose not to cast
  /// them, or not to receive them.
  bool get shadows => _shadows;
  set shadows(bool value) {
    _checkChangeable();
    final old = _shadows;
    _shadows = value;
    if (_write() case final failed?) {
      _shadows = old;
      failed.raise();
    }
  }

  @override
  void _put(GameObject object) => object._writeLight(this);

  @override
  void _takeOut(GameObject object) {
    check(fy_light_remove(object.world.pointer, object.id));
    _forget(object);
  }

  @override
  void _forget(GameObject object) => object.world._countLight(object, this, _LightCount.none);

  /// Writes it to the engine; what the engine refuses comes back, for the
  /// setter to undo its change and throw. Off, it's only kept here.
  _Failure? _write() {
    if (_refused() case final error?) return _Failure(error, StackTrace.current);
    if (!_enabledFlag) return null;
    try {
      _live?._writeLight(this);
      return null;
    } catch (error, stack) {
      return _Failure(error, stack);
    }
  }

  /// Why it can't be as it is, if it can't: the engine's ranges, then a
  /// shadowed spot light's width.
  ArgumentError? _refused() {
    final on = _owner;
    if (!(_intensity >= 0 && _intensity <= 1e6)) return _rangeError('Light.intensity', _intensity, 0, 1e6, owner: on);
    if (!(_range >= 0 && _range <= 1e6)) return _rangeError('Light.range', _range, 0, 1e6, unit: ' metres', owner: on);
    if (!(_angle >= -_slack && _angle <= 179 * _degree + _slack)) {
      return _rangeError('Light.angle', _angle, 0, 179, radians: true, owner: on);
    }
    if (!(_softness >= 0 && _softness <= 1)) return _rangeError('Light.softness', _softness, 0, 1, owner: on);
    if (_notFinite(_color)) return _colorError('Light.color', _color, on);
    if (_kind == LightKind.spot && _shadows && _angle > _widestShadowedSpot + 1e-9) {
      return ArgumentError(
        'a spot light casts shadows up to 150° across (its shadow map is at most 160°); this one'
            '${on == null ? '' : ' on $on'} is ${_angle.inDegrees.toStringAsFixed(1)}°. Narrow it, or turn its '
            'shadows off',
        'angle',
      );
    }
    return null;
  }

  @override
  String toString() {
    final more = shadows ? ', shadows: true' : '';
    return switch (kind) {
      LightKind.sun || LightKind.ambient => 'Light.${kind.name}(color: $color, intensity: $intensity$more)',
      LightKind.point => 'Light.point(color: $color, intensity: $intensity, range: $range$more)',
      LightKind.spot =>
        'Light.spot(color: $color, intensity: $intensity, range: $range, angle: ${angle.inDegrees}°, '
            'softness: $softness$more)',
    };
  }
}

/// Draws [mesh] with [material] at the object's [Transform]. Without both,
/// nothing is drawn. Change it in place: `get<MeshRenderer>()!.material = gold`.
///
/// A material is shared: every renderer that has it changes when it does.
/// For one object alone, set [color] (a hit flash, a team colour), or give
/// it its own material with [Material.copy].
///
/// It casts shadows and shadows fall on it, as in Unity; turn either off
/// with [castShadows] and [receiveShadows]
/// (`MeshRenderer(water, glass)..castShadows = false`).
final class MeshRenderer extends Component {
  /// Draws [mesh] with [material]: both are optional, and can be set
  /// later, but nothing is drawn until it has both.
  MeshRenderer([this._mesh, this._material]) : super._();

  Mesh? _mesh;
  Material? _material;
  Color? _color;
  bool _castShadows = true, _receiveShadows = true;

  // The materials of the mesh's parts after the first, and whether the
  // engine has some.
  List<Material>? _more;
  bool _moreSent = false;

  // Its numbers for its material's shader, its `PerObject`'s, in order.
  List<double>? _own;

  /// The shape it draws; null draws nothing. A mesh can be on any number
  /// of renderers.
  Mesh? get mesh => _mesh;
  set mesh(Mesh? value) {
    _checkChangeable();
    final old = _mesh;
    _mesh = value;
    if (_write() case final failed?) {
      _mesh = old;
      failed.raise();
    }
  }

  /// How its surface looks; null draws nothing. Shared: changing the
  /// material changes every object that uses it (as Unity's
  /// `sharedMaterial`, not its `material`, which Unity copies per
  /// renderer). For this object alone, set [color] or give it a
  /// [Material.copy].
  Material? get material => _material;
  set material(Material? value) {
    _checkChangeable();
    final old = _material;
    _material = value;
    if (_write() case final failed?) {
      _material = old;
      failed.raise();
    }
  }

  /// One material for each part of its mesh (Unity's `sharedMaterials`):
  /// the first is [material], and draws the first part; each after it
  /// draws the next part ([Mesh.setTriangles]' `submeshStarts`). A part
  /// without one isn't drawn. A list of one is [material] alone.
  List<Material> get materials => List.unmodifiable([?_material, ...?_more]);
  set materials(List<Material> value) {
    _checkChangeable();
    final (oldMaterial, oldMore) = (_material, _more);
    _material = value.isEmpty ? null : value.first;
    _more = value.length > 1 ? List.unmodifiable(value.skip(1)) : null;
    if (_write() case final failed?) {
      _material = oldMaterial;
      _more = oldMore;
      failed.raise();
    }
  }

  /// This object's own base colour, in place of its material's, which
  /// stays as it is for the other objects that share it (Unity's
  /// `MaterialPropertyBlock`). Null, as at first, for the material's.
  /// `colorTo` tweens it.
  Color? get color => _color;
  set color(Color? value) {
    _checkChangeable();
    final old = _color;
    _color = value;
    if (_write() case final failed?) {
      _color = old;
      failed.raise();
    }
  }

  /// Whether it blocks the light of lights with [Light.shadows]: true
  /// unless set. Off for what shouldn't darken the ground: water, a glow,
  /// a ghost.
  bool get castShadows => _castShadows;
  set castShadows(bool value) {
    _checkChangeable();
    final old = _castShadows;
    _castShadows = value;
    if (_write() case final failed?) {
      _castShadows = old;
      failed.raise();
    }
  }

  /// Whether shadows fall on it: true unless set.
  bool get receiveShadows => _receiveShadows;
  set receiveShadows(bool value) {
    _checkChangeable();
    final old = _receiveShadows;
    _receiveShadows = value;
    if (_write() case final failed?) {
      _receiveShadows = old;
      failed.raise();
    }
  }

  /// Sets this object's own number [name], one of its material's
  /// shader's `PerObject` ([Shader.perObject]), leaving the other objects
  /// that share the material as they are: a hit flash, a dissolve, a
  /// team's tint. 0 until set. Throws an [ArgumentError] for a name the
  /// shader doesn't have, or a number that isn't finite, and a
  /// [StateError] without a material with a shader. To tween one:
  /// `tween(1.0, 0.0, seconds: 0.2, onUpdate: (v) => renderer.set('flash', v))`.
  void set(String name, double value) {
    _checkChangeable();
    final index = _ownIndex(name);
    if (!value.isFinite) throw ArgumentError.value(value, name, 'must be finite');
    final own = _own ??= [0, 0, 0, 0];
    final old = own[index];
    own[index] = value;
    if (_write() case final failed?) {
      own[index] = old;
      failed.raise();
    }
  }

  /// This object's own number [name] ([set]): 0 until set.
  double number(String name) => _own?[_ownIndex(name)] ?? 0;

  int _ownIndex(String name) {
    final shader = _material?.shader;
    if (shader == null) throw StateError('$this has no material with a shader for $name');
    final index = shader._layout.own.indexOf(name);
    if (index < 0) {
      final names = shader._layout.own.isEmpty ? 'none' : shader._layout.own.join(', ');
      throw ArgumentError.value(name, 'name', 'is no PerObject number of $shader, which has: $names');
    }
    return index;
  }

  @override
  void _put(GameObject object) => object._writeMeshRenderer(this);

  @override
  void _takeOut(GameObject object) {
    final w = object.world.pointer;
    // Its parts' materials go with it, so they can be let go.
    if (_moreSent) {
      check(fy_submesh_materials_set(w, object.id, nullptr, 0));
      _moreSent = false;
    }
    check(fy_mesh_renderer_remove(w, object.id));
  }

  @override
  void _forget(GameObject object) => _moreSent = false;

  /// Writes it to the engine; what it or the engine refuses comes back,
  /// for the setter to undo its change and throw. Off, it's only kept
  /// here.
  _Failure? _write() {
    if (_color case final color? when _notFinite(color)) {
      return _Failure(_colorError('MeshRenderer.color', color, _owner), StackTrace.current);
    }
    if (!_enabledFlag) return null;
    try {
      _live?._writeMeshRenderer(this);
      return null;
    } catch (error, stack) {
      return _Failure(error, stack);
    }
  }

  @override
  String toString() => _color == null ? 'MeshRenderer($mesh, $material)' : 'MeshRenderer($mesh, $material, $_color)';
}

/// The game's own code on an object: extend it, and put it on any number
/// of objects, one instance each (`GameObject(components: [Health(100)])`
/// or [GameObject.addComponent]). An object has any number of behaviours,
/// of the same class or not.
///
/// **A behaviour is what an object does; a subclass of [GameObject] is
/// what it is.** Make a behaviour of code that other kinds of object could
/// use too (`Health` on the player and the enemies, `Spin` on anything),
/// or that comes and goes while the game runs. An object's own state and
/// parts go in its subclass. A subclass reads its behaviours with
/// [GameObject.get] (`Health get health => get<Health>()!;`); a behaviour
/// reaches its object as [gameObject], and what's above it with
/// [ancestor] (`ancestor<Level>()`).
///
/// A behaviour gets the same calls as its object, right after the
/// object's own, in the order the behaviours were added: [onLoad] when
/// it's put on a live object (or with its object), [onEnable] when it's
/// loaded and its object is shown, then [onStart], [onUpdate] and
/// [onLateUpdate] each frame, the pointer handlers ([onClick] and the
/// rest), [onDisable] when it or its object
/// is turned off, and [onDisable] and [onDestroy] when it's removed or its
/// object is destroyed. While [enabled] is false it gets none of them but
/// [onLoad] and [onDestroy], and it starts when it's next enabled.
abstract class Behaviour extends Component with _Moving {
  /// A behaviour on no object yet: give it to [GameObject] or
  /// [GameObject.addComponent].
  Behaviour() : super._();

  @override
  GameObject get _self => gameObject;

  @override
  GameObject? get _ownerObject => _owner;

  @override
  bool get _holdsStill => !_enabledNow;

  @override
  bool get _loading => !_loaded && !_removed;

  // A disabled behaviour waits to start until it's enabled. Setting
  // [enabled] calls [onDisable] or [onEnable] when that changes whether it's
  // enabled: only while it's loaded and its object is shown.
  @override
  void _enabledChanged(GameObject object) => object.world._refreshBehaviour(object, this);

  // Its load is done, it has had onStart, and it had onEnable and not
  // onDisable since.
  bool _loaded = false;
  bool _started = false;
  bool _enabledNow = false;

  // Its onDestroy is running, or has run: it listens and starts no more.
  bool get _going => _ending || _removed || gameObject._destroyed;
  bool _ending = false;

  /// The world its object is in. Throws a [StateError] before the object
  /// is added.
  World get world => gameObject.world;

  /// What the player does: the world's [World.input].
  Input get input => gameObject.input;

  /// The world's [World.debug]: lines drawn over the scene.
  Debug get debug => gameObject.debug;

  /// The world's clock: `time.delta` is the seconds this frame counts for.
  /// See [Time].
  Time get time => gameObject.time;

  /// Its object's assets: what it loads is held until the object is
  /// destroyed. See [GameObject.assets].
  Assets get assets => gameObject.assets;

  /// Starts a routine, as [GameObject.startRoutine] does; it stops when this
  /// behaviour is removed or its object destroyed, and is paused while
  /// the behaviour isn't enabled, though not during its own load.
  Routine<T> startRoutine<T>(Future<T> Function() body) {
    if (_going) throw StateError('$this was removed, so it can\'t start a routine');
    return gameObject.world._start(this, gameObject, body);
  }

  /// Stops every routine this behaviour started (Unity's
  /// `StopAllCoroutines`).
  void stopAllRoutines() => gameObject.world._stopRoutines(_routines);

  /// Listens to [signal], as [GameObject.on] does, until this behaviour
  /// is removed or its object destroyed.
  SignalSubscription<T> on<T>(Signal<T> signal, void Function(T value) handler) {
    if (_going) throw StateError('$this was removed, so it can\'t listen');
    return _on(this, signal, handler);
  }

  /// Called once, when it's put on a live object, or when its object is
  /// added: where it gets its assets. Return a future to load something.
  /// If its object is still loading, the object appears only when this is
  /// done too, and a load that throws fails the object's; on an object
  /// that has appeared, a load that throws removes only the behaviour and
  /// goes to [World.errors].
  @mustCallSuper
  @protected
  FutureOr<void> onLoad() {}

  /// Called each time it becomes enabled: loaded, [enabled], and its
  /// object shown (right after the object's own [GameObject.onEnable]).
  @mustCallSuper
  @protected
  void onEnable() {}

  /// Called once, in the first [World.update] where it's loaded, enabled
  /// and its object is shown, before its first [onUpdate].
  @mustCallSuper
  @protected
  void onStart() {}

  /// Called every fixed step, right after its object's
  /// [GameObject.onFixedUpdate], while it's enabled and its object is
  /// shown: for forces and what must not depend on the frame rate.
  @mustCallSuper
  @protected
  void onFixedUpdate() {}

  /// Called every [World.update], right after its object's, while it's
  /// enabled and its object is shown. Move by `time.delta`.
  @mustCallSuper
  @protected
  void onUpdate() {}

  /// Called every [World.update] after every [onUpdate], right after its
  /// object's [GameObject.onLateUpdate].
  @mustCallSuper
  @protected
  void onLateUpdate() {}

  /// Called each time it stops being enabled: [enabled] set false, its
  /// object hidden, or right before [onDestroy].
  @mustCallSuper
  @protected
  void onDisable() {}

  /// Called when it's removed, or its object is destroyed (right after the
  /// object's own), while the object is still there to read.
  @mustCallSuper
  @protected
  void onDestroy() {}

  /// Called when the app goes away (true) and comes back (false), while
  /// it's enabled, right after its object's
  /// [GameObject.onApplicationPause].
  @mustCallSuper
  @protected
  void onApplicationPause(bool paused) {}

  // Pointers: each is its object's (GameObject.onPointerDown and the
  // rest), right after the object's own; one that takes a down or a drag
  // gets its up, or the rest of the drag, itself.

  /// See [GameObject.onPointerDown].
  @protected
  bool onPointerDown(Finger finger) => false;

  /// See [GameObject.onPointerUp].
  @protected
  void onPointerUp(Finger finger) {}

  /// See [GameObject.onPointerEnter].
  @protected
  void onPointerEnter(Finger finger) {}

  /// See [GameObject.onPointerExit].
  @protected
  void onPointerExit(Finger finger) {}

  /// See [GameObject.onClick].
  @protected
  bool onClick(Finger finger) => false;

  /// See [GameObject.onLongPress].
  @protected
  bool onLongPress(Finger finger) => false;

  /// See [GameObject.onDragStart].
  @protected
  bool onDragStart(Finger finger) => false;

  /// See [GameObject.onDrag].
  @protected
  void onDrag(Finger finger) {}

  /// See [GameObject.onDragEnd].
  @protected
  void onDragEnd(Finger finger) {}

  @override
  String toString() => _owner == null ? '$runtimeType' : '$runtimeType on $_owner';
}

/// What a component's write threw: its setter puts the old value back,
/// then throws it. Made only when the engine refuses a value.
final class _Failure {
  _Failure(this.error, this.stack);

  final Object error;
  final StackTrace stack;

  Never raise() => Error.throwWithStackTrace(error, stack);
}

// What the engine checks of a component, checked in Dart where the value
// is given, in the game's names and units (stage 14, C1 and C2): the
// engine's own check stays behind it. Each test is written out in place,
// as a double passed to a call the JIT doesn't inline is boxed, and a
// light or a colour set each frame allocates nothing; these make the
// message once a test fails.

/// A degree, in radians, and a little more for the last digits a value
/// in degrees loses on its way to radians and back.
const _degree = math.pi / 180, _slack = 1e-9;

/// [v] for a message: a whole number without its `.0`, another to four
/// significant digits.
String _valueText(double v) {
  if (!v.isFinite) return '$v';
  if (v == v.roundToDouble() && v.abs() < 1e15) return '${v.round()}';
  return '${double.parse(v.toStringAsPrecision(4))}';
}

/// The error for [value], given as [what] (`Camera.near`) to a component
/// on [owner]: NaN, or outside [min] to [max] (in [unit]). An angle in
/// [radians] has its range in degrees, and one that looks like degrees is
/// asked about: `did you mean 60.deg?`.
ArgumentError _rangeError(
  String what,
  double value,
  double min,
  double max, {
  String unit = '',
  bool radians = false,
  GameObject? owner,
}) {
  final on = owner == null ? '' : ' on $owner';
  final name = what.substring(what.lastIndexOf('.') + 1);
  if (!radians) {
    final range = '${_valueText(min)} to ${_valueText(max)}$unit';
    final why = value.isFinite ? 'it goes from $range' : 'give a number from $range';
    return ArgumentError('$what$on is ${_valueText(value)}: $why', name);
  }
  final range = '${_valueText(min)}° to ${_valueText(max)}° (${_valueText(min.deg)} to ${_valueText(max.deg)})';
  final shown = _valueText(value);
  final hint = !value.isFinite
      ? ''
      : ', and $shown radians is ${_valueText(value.inDegrees)}°'
            '${value >= min && value <= max ? ': did you mean $shown.deg?' : ''}';
  return ArgumentError('$what$on is $shown, in radians, from $range$hint', name);
}

/// Whether a part of [color] is NaN or infinite (as a 32-bit float, as
/// the engine keeps it).
bool _notFinite(Color color) =>
    !(color.r.abs() < 3.4e38 && color.g.abs() < 3.4e38 && color.b.abs() < 3.4e38 && color.a.abs() < 3.4e38);

/// The error for [color], not finite, given as [what] to a component on
/// [owner].
ArgumentError _colorError(String what, Color color, GameObject? owner) {
  final on = owner == null ? '' : ' on $owner';
  return ArgumentError('$what$on is $color: NaN or infinity', what.substring(what.lastIndexOf('.') + 1));
}
