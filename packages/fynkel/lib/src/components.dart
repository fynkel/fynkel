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
  T? getInParent<T extends Object>() => gameObject.getInParent<T>();

  /// The first component of type [T] on its object or below it: see
  /// [GameObject.getInChildren].
  T? getInChildren<T extends Object>() => gameObject.getInChildren<T>();

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

/// Makes the object a camera. The view is drawn from the shown object
/// with a camera and the lowest id ([World.camera]). Change it in place:
/// `get<Camera>()!.fovY = 40.deg`.
///
/// A value the engine refuses (out of its range, NaN) throws an
/// [ArgumentError] from the setter and leaves the old value in place.
final class Camera extends Component {
  /// A perspective camera with a 60° [fovY], [near] 0.1 m, [far] 1,000 m,
  /// a dark grey [clearColor], drawing every layer. Pass [orthographic]
  /// for a parallel one, [orthographicSize] 5 unless given.
  Camera({
    this._fovY = 60 * (math.pi / 180),
    this._near = 0.1,
    this._far = 1000,
    this._clearColor = const Color.rgb(0.1, 0.1, 0.12),
    this._orthographic = false,
    this._orthographicSize = 5,
    this._cullingMask = LayerMask.all,
  }) : super._();

  double _fovY, _near, _far, _orthographicSize;
  Color _clearColor;
  bool _orthographic;
  LayerMask _cullingMask;

  /// Vertical field of view in radians, 1° to 179°: 60.deg unless set.
  double get fovY => _fovY;
  set fovY(double value) => _change(() => _fovY = value);

  /// How near to the camera things are drawn, in metres (Unity's
  /// `nearClipPlane`): 0.1 unless set, 0.0001 to 1,000,000. Keep it below
  /// [far]; nothing checks that.
  double get near => _near;
  set near(double value) => _change(() => _near = value);

  /// How far from the camera things are drawn, in metres (Unity's
  /// `farClipPlane`): 1,000 unless set, 0.001 to 1,000,000,000.
  double get far => _far;
  set far(double value) => _change(() => _far = value);

  /// What the view shows where nothing is drawn (Unity's
  /// `backgroundColor`).
  Color get clearColor => _clearColor;
  set clearColor(Color value) => _change(() => _clearColor = value);

  /// Parallel projection: things keep their size however far away they
  /// are, and [fovY] is unused. For a map, a board game or a side view.
  bool get orthographic => _orthographic;
  set orthographic(bool value) => _change(() => _orthographic = value);

  /// Half the view's height, in metres, when [orthographic] (Unity's
  /// `orthographicSize`): 5 shows 10 m from top to bottom.
  double get orthographicSize => _orthographicSize;
  set orthographicSize(double value) => _change(() => _orthographicSize = value);

  /// The layers it draws (Unity's `cullingMask`): objects on the others
  /// ([GameObject.layer]) aren't drawn, nor tapped through it. Lights light
  /// every layer.
  LayerMask get cullingMask => _cullingMask;
  set cullingMask(LayerMask value) => _change(() => _cullingMask = value);

  /// The ray from this camera through [point] in a view showing it: from
  /// the point on the near plane, away from the camera. Cast it with
  /// [World.raycast] to find what's drawn there (Unity's
  /// `ScreenPointToRay`).
  ///
  /// Uses its object's [GameObject.worldTransform] (its scale is ignored,
  /// as the view ignores it). Throws a [StateError] when it's on no object.
  Ray screenPointToRay(ViewPoint point) {
    if (!(point.width > 0 && point.height > 0)) {
      throw ArgumentError.value(point, 'point', 'needs a view with an area');
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
    if (!(w > 0 && h > 0)) throw ArgumentError('worldToScreenPoint needs a view with an area; got ${w}x$h');
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

  /// Makes a change and writes it to the engine; undoes it if the engine
  /// refuses it.
  void _change(void Function() set) {
    _checkChangeable();
    final (fovY, near, far, clearColor, orthographic, size, cullingMask) = (
      _fovY,
      _near,
      _far,
      _clearColor,
      _orthographic,
      _orthographicSize,
      _cullingMask,
    );
    set();
    try {
      _live?._writeCamera(this);
    } catch (_) {
      _fovY = fovY;
      _near = near;
      _far = far;
      _clearColor = clearColor;
      _orthographic = orthographic;
      _orthographicSize = size;
      _cullingMask = cullingMask;
      rethrow;
    }
  }

  @override
  String toString() =>
      'Camera(${orthographic ? 'orthographic, orthographicSize: $orthographicSize' : 'fovY: $fovY'}, near: $near, far: $far, '
      'clearColor: $clearColor${cullingMask == LayerMask.all ? '' : ', cullingMask: $cullingMask'})';
}

/// What kind of light a [Light] is.
enum LightKind {
  /// Light from far away, like the sun, shining along the object's forward
  /// (its [GameObject.forward]): Unity's `LightType.Directional`. Only the
  /// one with the lowest id is used.
  sun,

  /// Light from every direction, adding to every surface, as Unity's
  /// `RenderSettings.ambientLight`. All ambient lights add up.
  ambient,
}

/// Lights the scene. Change it in place: `get<Light>()!.intensity = 2`.
final class Light extends Component {
  /// Shines along the object's [GameObject.forward]: give the object
  /// `Transform(rotation: Quat.lookRotation(direction))`.
  Light.sun({Color color = Color.white, double intensity = 1}) : this._(LightKind.sun, color, intensity);

  /// Light from every direction: see [LightKind.ambient]. A dim one, say
  /// `Light.ambient(intensity: 0.25)`, keeps the sides the sun misses from
  /// going black.
  Light.ambient({Color color = Color.white, double intensity = 1}) : this._(LightKind.ambient, color, intensity);

  Light._(this._kind, this._color, this._intensity) : super._();

  LightKind _kind;
  Color _color;
  double _intensity;

  /// A sun or an ambient light; it can change while it's on.
  LightKind get kind => _kind;
  set kind(LightKind value) => _change(() => _kind = value);

  /// The light's colour: white unless set.
  Color get color => _color;
  set color(Color value) => _change(() => _color = value);

  /// How bright it is, 0 to 1,000,000; 1 unless set. A sun of 1 lights a
  /// white, rough surface that faces it to exactly its material's colour.
  /// Out of range or NaN throws an [ArgumentError] and keeps the old
  /// value.
  double get intensity => _intensity;
  set intensity(double value) => _change(() => _intensity = value);

  void _change(void Function() set) {
    _checkChangeable();
    final (kind, color, intensity) = (_kind, _color, _intensity);
    set();
    try {
      _live?._writeLight(this);
    } catch (_) {
      _kind = kind;
      _color = color;
      _intensity = intensity;
      rethrow;
    }
  }

  @override
  String toString() => 'Light.${kind.name}(color: $color, intensity: $intensity)';
}

/// Draws [mesh] with [material] at the object's [Transform]. Without both,
/// nothing is drawn. Change it in place: `get<MeshRenderer>()!.material = gold`.
///
/// A material is shared: every renderer that has it changes when it does.
/// For one object alone, set [color] (a hit flash, a team colour), or give
/// it its own material with [Material.copy].
final class MeshRenderer extends Component {
  /// Draws [mesh] with [material]: both are optional, and can be set
  /// later, but nothing is drawn until it has both.
  MeshRenderer([this._mesh, this._material]) : super._();

  Mesh? _mesh;
  Material? _material;
  Color? _color;

  /// The shape it draws; null draws nothing. A mesh can be on any number
  /// of renderers.
  Mesh? get mesh => _mesh;
  set mesh(Mesh? value) => _change(() => _mesh = value);

  /// How its surface looks; null draws nothing. Shared: changing the
  /// material changes every object that uses it (as Unity's
  /// `sharedMaterial`, not its `material`, which Unity copies per
  /// renderer). For this object alone, set [color] or give it a
  /// [Material.copy].
  Material? get material => _material;
  set material(Material? value) => _change(() => _material = value);

  /// This object's own base colour, in place of its material's, which
  /// stays as it is for the other objects that share it (Unity's
  /// `MaterialPropertyBlock`). Null, as at first, for the material's.
  /// `colorTo` tweens it.
  Color? get color => _color;
  set color(Color? value) => _change(() => _color = value);

  void _change(void Function() set) {
    _checkChangeable();
    final (mesh, material, color) = (_mesh, _material, _color);
    set();
    try {
      _live?._writeMeshRenderer(this);
    } catch (_) {
      _mesh = mesh;
      _material = material;
      _color = color;
      rethrow;
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

  /// Whether it gets its object's frames and pointers. A disabled behaviour
  /// waits to start until it's enabled. Setting it calls [onDisable] or
  /// [onEnable] when that changes whether it's enabled: only while it's
  /// loaded and its object is shown.
  bool get enabled => _enabledFlag;
  set enabled(bool value) {
    if (value == _enabledFlag) return;
    _enabledFlag = value;
    if (_live case final object?) object.world._refreshBehaviour(object, this);
  }

  bool _enabledFlag = true;

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
  /// the behaviour isn't enabled.
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
