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

  /// Called at the start of each fixed step, before any
  /// [GameObject.onFixedUpdate].
  @mustCallSuper
  void beforeFixedUpdate() {}

  /// Called at the end of each fixed step, after every
  /// [GameObject.onFixedUpdate] and before the routines waiting for
  /// [fixedStep]: physics steps here.
  @mustCallSuper
  void afterFixedUpdate() {}

  /// Called every frame after the late updates, before the cameras move.
  @mustCallSuper
  void afterLateUpdate() {}

  /// Called once when the world is disposed: free what the system holds.
  @mustCallSuper
  void dispose() {}
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
}
