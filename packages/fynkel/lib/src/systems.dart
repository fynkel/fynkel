part of 'world.dart';

// World systems (docs/stage-5.md §3.1): what a package such as
// package:fynkel_physics hangs on a world's frame. Games use them through
// the package; they're in package:fynkel/extend.dart, not fynkel.dart.

/// Work a package adds to a world's frame, one of each type per world:
/// [World.system] makes it the first time it's asked for. Its hooks run in
/// [World.update], systems by their [order], then in the order they were
/// made; they run while the world is [World.paused] too. What a hook
/// throws goes to [World.errors] with no object, and the frame goes on; a
/// system whose hooks throw in [turnOffAfter] frames in a row is turned
/// off ([isTurnedOff]), and that's reported once.
abstract class WorldSystem {
  /// A system for [world]. Make it in the function given to
  /// [World.system] (`world.system(Wind.new)`, for a `Wind` system of
  /// the game's or a package's), which adds it; one made
  /// any other way is never run.
  WorldSystem(this.world);

  /// The world whose frame it works in.
  final World world;

  /// After how many frames in a row with a hook throwing a system is
  /// turned off.
  static const turnOffAfter = 5;

  /// Where its hooks run among the world's systems: a lower order first,
  /// and systems with the same order in the order they were made. Read
  /// once, when it's made. 0 unless overridden. The built-in animators,
  /// then the particles, come first of all ([afterUpdate]), and the
  /// cameras last ([afterLateUpdate]).
  int get order => 0;

  /// What [World.profile] calls it: its type's name unless overridden.
  String get name => '$runtimeType';

  /// How long its hooks took in the last frame, all of them together
  /// ([World.profile]).
  Duration get frameTime => world.profile._duration(_lastTicks);

  // The ticks its hooks took this frame, and in the last finished one.
  var _ticks = 0;
  var _lastTicks = 0;

  /// Whether its hooks threw in [turnOffAfter] frames in a row: none of
  /// them runs again, but for [dispose].
  bool get isTurnedOff => _off;
  var _off = false;

  // The last frame ([Time.frame]) a hook threw in, and how many frames in
  // a row up to it one did.
  var _threwIn = -1;
  var _throwingFrames = 0;

  /// Called at the start of each frame, after the time moves on and
  /// before the input and the starts: physics collects the step it ran
  /// between frames here.
  @protected
  @mustCallSuper
  void beforeFrame() {}

  /// Called at the start of each fixed step, before any
  /// [GameObject.onFixedUpdate].
  @protected
  @mustCallSuper
  void beforeFixedUpdate() {}

  /// Called at the end of each fixed step, after every
  /// [GameObject.onFixedUpdate] and before the routines waiting for
  /// [fixedStep]: physics steps here.
  @protected
  @mustCallSuper
  void afterFixedUpdate() {}

  /// Called every frame after the fixed steps, before any
  /// [GameObject.onUpdate]: physics puts interpolated bodies here.
  @protected
  @mustCallSuper
  void beforeUpdate() {}

  /// Called every frame after every [GameObject.onUpdate], the motions and
  /// the routines, before any [GameObject.onLateUpdate]: where an animator
  /// poses what the updates moved, as Unity's does.
  @protected
  @mustCallSuper
  void afterUpdate() {}

  /// Called every frame after the late updates, before the cameras move.
  @protected
  @mustCallSuper
  void afterLateUpdate() {}

  /// Called at the end of each frame, after everything else: physics
  /// starts the step it runs between frames here.
  @protected
  @mustCallSuper
  void afterFrame() {}

  /// Called when [component] comes into the world on [object]: put on it
  /// while it's in the world, or with it when it's added (each of an added
  /// tree's components, parents first). After the engine has it, before a
  /// behaviour loads. Not for a model's parts, read from the model.
  @protected
  @mustCallSuper
  void componentAdded(GameObject object, Component component) {}

  /// Called when [component] leaves the world, for each [componentAdded]
  /// had: it's removed from [object], or [object] is destroyed (then
  /// [GameObject.isDestroyed] is true, and it's called for each of its
  /// components, after every `onDestroy`). The component is
  /// [Component.isRemoved] by then.
  @protected
  @mustCallSuper
  void componentRemoved(GameObject object, Component component) {}

  /// What [ray], cast from the camera through a finger, hits of what the
  /// system holds within [layers], or null: the input takes the nearest
  /// of the world's [World.raycast] and every system's, so a package can
  /// make what has no mesh tappable (physics: its colliders).
  RayHit? pointerHit(Ray ray, LayerMask layers) => null;

  /// Called once, when the world is first shown on screen (by a
  /// `FynkelView`; a [HeadlessView] doesn't count): audio opens the sound
  /// output here, so a test makes no sound. A system made after that isn't
  /// called; [WorldSystems.isOnScreen] says so.
  @protected
  @mustCallSuper
  void viewStarted() {}

  /// Called when [World.applicationPaused] changes, before the objects'
  /// [GameObject.onApplicationPause]: audio stops while the app is away.
  @protected
  @mustCallSuper
  void applicationPause(bool paused) {}

  /// Called once when the world is disposed: free what the system holds.
  ///
  /// A game may keep the system past this (`final physics =
  /// world.physics;` in a HUD), so every public call made afterwards must
  /// throw a [StateError] before it touches native memory, and scratch it
  /// writes into first must still be there: free it with a
  /// `NativeFinalizer` (when the system is collected, or the isolate ends
  /// on a hot restart), not here.
  @protected
  @mustCallSuper
  void dispose() {}
}

// The built-in systems' [WorldSystem.order]s: first and last of any a
// package's would sensibly have.
const _animatorsOrder = -1 << 40;
const _particlesOrder = _animatorsOrder + 1;
const _camerasOrder = 1 << 40;

/// What a system that follows objects reads of each.
extension GameObjectSystems on GameObject {
  /// The [World.transformVersion] when this object's transform or parent
  /// was last set, or 0. While it stays the same, nothing set this object's
  /// transform; its world transform also changes when an ancestor's does.
  int get transformStamp => _transformStamp;

  /// How many components it holds, removed ones among them until the
  /// world tidies them away: read them with [componentAt], which, unlike
  /// [GameObject.getAll], makes no list.
  int get componentCount => _components?.length ?? 0;

  /// Its component at [index], below [componentCount]; it may be
  /// [Component.isRemoved].
  Component componentAt(int index) => _components![index];

  // Its children are read with GameObject.childCount and childAt, a
  // game's too.
}

/// Finding and making a world's systems.
extension WorldSystems on World {
  /// The world's system of type [T], made with [make] the first time.
  /// Throws a [StateError] if the world was disposed.
  T system<T extends WorldSystem>(T Function(World world) make) {
    if (isDisposed) throw StateError('this World was disposed');
    if (_systemsByType[T] case final T system) return system;
    final system = make(this);
    _systemsByType[T] = system;
    // A new list, so a hook running over the old one, which made this
    // system, runs no system twice.
    final order = system.order;
    var at = _systems.length;
    while (at > 0 && _systems[at - 1].order > order) {
      at--;
    }
    _systems = [..._systems]..insert(at, system);
    return system;
  }

  /// Lets [Assets.preload] load files ending in [kind]'s
  /// [AssetKind.extensions]; [Assets.load] needs no adding. Throws an
  /// [ArgumentError] if another kind has one of them.
  void addAssetKind(AssetKind<Object> kind) {
    if (isDisposed) throw StateError('this World was disposed');
    for (final known in _assetKinds) {
      if (identical(known, kind)) return;
      for (final extension in kind.extensions) {
        if (known.extensions.contains(extension.toLowerCase())) {
          throw ArgumentError('the ${kind.name} kind loads $extension, which ${known.name}s already do', 'kind');
        }
      }
    }
    _assetKinds.add(kind);
  }

  /// Whether the world has been shown on screen ([WorldSystem.viewStarted]).
  bool get isOnScreen => _onScreen;

  /// The world's system of type [T], or null if it has none yet.
  T? maybeSystem<T extends WorldSystem>() => _systemsByType[T] as T?;

  /// Sends what a game's code threw from a call a system made (a physics
  /// callback, say) to [World.errors], for [object] if it's one's: the
  /// system goes on.
  void report(GameObject? object, Object error, StackTrace stack) => _report(object, error, stack);

  /// A number that goes up whenever an object's transform, parent or layer
  /// is set. While it stays the same, no object's world transform or layer
  /// has changed, so a system that follows objects has nothing to read.
  int get transformVersion => _transformVersion;

  /// A new reader of the world's moved log, which starts reading here:
  /// every object whose transform, parent or layer is set from now on is
  /// in it, at least once, until the reader reads past it. [MovedReader.close]
  /// it when done.
  MovedReader movedReader() {
    if (isDisposed) throw StateError('this World was disposed');
    final reader = MovedReader._(this);
    _movedReaders.add(reader);
    return reader;
  }

  /// A number that goes up whenever an object is added, destroyed, shown,
  /// hidden or given a new parent: while it stays the same, every object's
  /// parent is what it was.
  int get hierarchyVersion => _shape;

  /// Reads the world transforms of [count] objects, by their [GameObject.id]s
  /// at [ids], into [out]: ten 32-bit floats each, the position (x, y, z),
  /// the rotation (x, y, z, w) and the scale (x, y, z), as the engine
  /// works them out (from its matrices: [GameObject.worldTransform]'s, but
  /// for a rotation under an uneven scale). One call however many there
  /// are; the moves set since the last send are sent first. Throws an
  /// [ArgumentError] if an id isn't a live object.
  void readWorldTransforms(Pointer<Uint64> ids, int count, Pointer<Float> out) {
    if (count == 0) return;
    _sendTransforms();
    // Each system reads into its own buffer, the same each time, as a
    // rule: each buffer's cast is kept, since making one each call
    // allocates.
    Pointer<FyTransform>? cast;
    for (var i = 0; i < _manyOuts.length; i++) {
      if (_manyOuts[i].address == out.address) cast = _manyOuts[i];
    }
    if (cast == null) {
      if (_manyOuts.length == 8) _manyOuts.removeAt(0);
      cast = out.cast();
      _manyOuts.add(cast);
    }
    final status = fy_world_transform_get_many(pointer, ids, count, cast);
    if (status == FyStatus.FY_STATUS_NO_OBJECT) {
      throw ArgumentError(fy_last_error().cast<Utf8>().toDartString(), 'ids');
    }
    check(status);
  }

  /// Sets the transforms of [count] objects, by their [GameObject.id]s at
  /// [ids], from [transforms]: ten 32-bit floats each, as
  /// [readWorldTransforms] writes them, but each relative to its object's
  /// parent, as [GameObject.transform] is. For a package that works out
  /// many poses natively (an animator, particles): one call however many
  /// there are, each sent with the frame's other moves. Throws an
  /// [ArgumentError] if an id isn't a live object or a transform isn't
  /// valid; those before it are set.
  void writeTransforms(Pointer<Uint64> ids, int count, Pointer<Float> transforms) {
    pointer;
    for (var i = 0; i < count; i++) {
      final id = ids[i];
      final object = _objects[id];
      if (object == null || object._destroyed) {
        throw ArgumentError.value(id, 'ids[$i]', 'not a live object of this world');
      }
      final f = transforms + i * 10;
      object.transform = Transform._(Vec3(f[0], f[1], f[2]), Quat(f[3], f[4], f[5], f[6]), Vec3(f[7], f[8], f[9]));
    }
  }
}

/// A system's place in its world's moved log ([WorldSystems.movedReader]):
/// the objects whose transform, parent or layer was set since it last
/// read, in the order they were first set. Each is in it at least once:
/// an object set many times is in it again only once a reader has read
/// past it. Their world transforms changed, and so did those of every
/// object below them; they may since have been destroyed.
///
/// Read the objects by index, which makes no list, then [markRead]:
///
/// ```dart
/// for (var i = 0; i < moved.length; i++) {
///   final object = moved[i];
///   if (!object.isDestroyed) follow(object);
/// }
/// moved.markRead();
/// ```
///
/// The log keeps an entry until every reader has read it, so each reader
/// should read now and then; an object waits in it once however often it
/// moves meanwhile.
final class MovedReader {
  MovedReader._(this._world) : _cursor = _world._movedBase + _world._movedLog.length {
    // What's in the log is behind this reader: an object there moving
    // again must be put in it again, for this one.
    if (_cursor > _world._movedReadTo) _world._movedReadTo = _cursor;
  }

  final World _world;
  int _cursor;
  var _closed = false;

  /// How many objects moved since the last [markRead].
  int get length => _closed ? 0 : _world._movedBase + _world._movedLog.length - _cursor;

  /// The object at [index], below [length].
  GameObject operator [](int index) {
    RangeError.checkValidIndex(index, this, 'index', length);
    return _world._movedLog[_cursor - _world._movedBase + index];
  }

  /// Marks every object up to now read: [length] is 0 until another moves.
  void markRead() {
    if (_closed) return;
    final world = _world;
    _cursor = world._movedBase + world._movedLog.length;
    if (_cursor > world._movedReadTo) world._movedReadTo = _cursor;
    world._trimMoved();
  }

  /// Stops reading: the log no longer keeps entries for this reader.
  void close() {
    if (_closed) return;
    _closed = true;
    _world._movedReaders.remove(this);
    _world._trimMoved();
  }
}

extension _MovedLog on World {
  /// Puts [object] in the moved log, unless no reader reads it, or it's
  /// already there for every reader.
  void _logMoved(GameObject object) {
    if (_movedReaders.isEmpty || object._loggedAt >= _movedReadTo) return;
    object._loggedAt = _movedBase + _movedLog.length;
    _movedLog.add(object);
  }

  /// Lets go of the entries every reader has read: all of them, or none.
  void _trimMoved() {
    final end = _movedBase + _movedLog.length;
    for (var i = 0; i < _movedReaders.length; i++) {
      if (_movedReaders[i]._cursor != end) return;
    }
    _movedBase = end;
    _movedLog.clear();
  }
}
