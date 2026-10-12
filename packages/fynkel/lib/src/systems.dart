part of 'world.dart';

// World systems (docs/stage-5.md §3.1): what a package such as
// package:fynkel_physics hangs on a world's frame. Games use them through
// the package; they're in package:fynkel/extend.dart, not fynkel.dart.

/// Work a package adds to a world's frame, one of each type per world:
/// [World.system] makes it the first time it's asked for. Its hooks run in
/// [World.update], systems in the order they were made; what they throw
/// goes to [World.errors] with no object, and the frame goes on.
abstract class WorldSystem {
  WorldSystem(this.world);

  final World world;

  /// Called at the start of each frame, after the time moves on and
  /// before the input and the starts: physics collects the step it ran
  /// between frames here.
  @mustCallSuper
  void beforeFrame() {}

  /// Called at the start of each fixed step, before any
  /// [GameObject.onFixedUpdate].
  @mustCallSuper
  void beforeFixedUpdate() {}

  /// Called at the end of each fixed step, after every
  /// [GameObject.onFixedUpdate] and before the routines waiting for
  /// [fixedStep]: physics steps here.
  @mustCallSuper
  void afterFixedUpdate() {}

  /// Called every frame after the fixed steps, before any
  /// [GameObject.onUpdate]: physics puts interpolated bodies here.
  @mustCallSuper
  void beforeUpdate() {}

  /// Called every frame after the late updates, before the cameras move.
  @mustCallSuper
  void afterLateUpdate() {}

  /// Called at the end of each frame, after everything else: physics
  /// starts the step it runs between frames here.
  @mustCallSuper
  void afterFrame() {}

  /// Called when [component] is put on [object] while it's in the world,
  /// after the engine has it and before a behaviour loads.
  @mustCallSuper
  void componentAdded(GameObject object, Component component) {}

  /// Called once when the world is disposed: free what the system holds.
  ///
  /// A game may keep the system past this (`final physics =
  /// world.physics;` in a HUD), so every public call made afterwards must
  /// throw a [StateError] before it touches native memory, and scratch it
  /// writes into first must still be there: free it with a
  /// `NativeFinalizer` (when the system is collected, or the isolate ends
  /// on a hot restart), not here.
  @mustCallSuper
  void dispose() {}
}

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
    _systems.add(system);
    return system;
  }

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

  /// A number that goes up whenever an object is added, destroyed, shown,
  /// hidden or given a new parent: while it stays the same, every object's
  /// parent is what it was.
  int get hierarchyVersion => _shape;

  /// Reads the world transforms of [count] objects, by their [GameObject.id]s
  /// at [ids], into [out]: ten 32-bit floats each, the position (x, y, z),
  /// the rotation (x, y, z, w) and the scale (x, y, z), as
  /// [GameObject.worldTransform] gives them. One call however many there
  /// are; the moves set since the last send are sent first. Throws a
  /// [FynkelException] if an id isn't a live object.
  void readWorldTransforms(Pointer<Uint64> ids, int count, Pointer<Float> out) {
    if (count == 0) return;
    _sendTransforms();
    // The same buffer each time, as a rule: its cast is kept, since making
    // one each call allocates.
    if (out.address != _manyOut.address) _manyOut = out.cast();
    check(fy_world_transform_get_many(pointer, ids, count, _manyOut));
  }
}
