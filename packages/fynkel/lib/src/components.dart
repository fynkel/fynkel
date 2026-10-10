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
/// lowest id that also has a [Transform] ([World.camera]). Change it in
/// place: `get<Camera>()!.fovY = 40`.
final class Camera extends Component {
  Camera({
    this._fovY = 60,
    this._near = 0.1,
    this._far = 1000,
    this._clearColor = const Color.rgb(0.1, 0.1, 0.12),
    this._orthographic = false,
    this._size = 5,
    this._cullingMask = LayerMask.all,
  }) : super._();

  double _fovY, _near, _far, _size;
  Color _clearColor;
  bool _orthographic;
  LayerMask _cullingMask;

  /// Vertical field of view in degrees, 1–179.
  double get fovY => _fovY;
  set fovY(double value) => _change(() => _fovY = value);

  double get near => _near;
  set near(double value) => _change(() => _near = value);

  double get far => _far;
  set far(double value) => _change(() => _far = value);

  /// What the view shows where nothing is drawn.
  Color get clearColor => _clearColor;
  set clearColor(Color value) => _change(() => _clearColor = value);

  /// Parallel projection: things keep their size however far away they
  /// are, and [fovY] is unused. For a map, a board game or a side view.
  bool get orthographic => _orthographic;
  set orthographic(bool value) => _change(() => _orthographic = value);

  /// Half the view's height, in metres, when [orthographic] (Unity's
  /// `orthographicSize`): 5 shows 10 m from top to bottom.
  double get size => _size;
  set size(double value) => _change(() => _size = value);

  /// The layers it draws (Unity's `cullingMask`): objects on the others
  /// ([GameObject.layer]) aren't drawn, nor tapped through it. Lights light
  /// every layer.
  LayerMask get cullingMask => _cullingMask;
  set cullingMask(LayerMask value) => _change(() => _cullingMask = value);

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
      _size,
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
      _size = size;
      _cullingMask = cullingMask;
      rethrow;
    }
  }

  @override
  String toString() =>
      'Camera(${orthographic ? 'orthographic, size: $size' : 'fovY: $fovY'}, near: $near, far: $far, '
      'clearColor: $clearColor${cullingMask == LayerMask.all ? '' : ', cullingMask: $cullingMask'})';
}

enum LightKind {
  /// Light from far away, like the sun, shining along the object's forward
  /// (its [Transform.forward]). Only the one with the lowest id is used.
  sun,

  /// Light from every direction, adding to every surface. All ambient
  /// lights add up.
  ambient,
}

/// Lights the scene. Change it in place: `get<Light>()!.intensity = 2`.
final class Light extends Component {
  /// Shines along the object's [Transform.forward]: give the object
  /// `Transform(rotation: Quat.lookRotation(direction))`.
  Light.sun({Color color = Color.white, double intensity = 1}) : this._(LightKind.sun, color, intensity);

  Light.ambient({Color color = Color.white, double intensity = 1}) : this._(LightKind.ambient, color, intensity);

  Light._(this._kind, this._color, this._intensity) : super._();

  LightKind _kind;
  Color _color;
  double _intensity;

  LightKind get kind => _kind;
  set kind(LightKind value) => _change(() => _kind = value);

  Color get color => _color;
  set color(Color value) => _change(() => _color = value);

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
final class MeshRenderer extends Component {
  MeshRenderer([this._mesh, this._material]) : super._();

  Mesh? _mesh;
  Material? _material;

  Mesh? get mesh => _mesh;
  set mesh(Mesh? value) => _change(() => _mesh = value);

  Material? get material => _material;
  set material(Material? value) => _change(() => _material = value);

  void _change(void Function() set) {
    _checkChangeable();
    final (mesh, material) = (_mesh, _material);
    set();
    try {
      _live?._writeMeshRenderer(this);
    } catch (_) {
      _mesh = mesh;
      _material = material;
      rethrow;
    }
  }

  @override
  String toString() => 'MeshRenderer($mesh, $material)';
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
  Behaviour() : super._();

  @override
  GameObject get _self => gameObject;

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

  // The routines it started that are still running, and the signals it
  // listens to with [on].
  List<Routine<Object?>>? _routines;
  _Listening? _listening;

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

  /// Starts a routine, as [GameObject.start] does; it stops when this
  /// behaviour is removed or its object destroyed, and is paused while
  /// the behaviour isn't enabled.
  Routine<T> start<T>(Future<T> Function() body) => gameObject.world._start(this, gameObject, body);

  /// Listens to [signal], as [GameObject.on] does, until this behaviour
  /// is removed or its object destroyed.
  SignalSubscription<T> on<T>(Signal<T> signal, void Function(T value) handler) {
    if (_removed) throw StateError('$this was removed, so it can\'t listen');
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
