part of 'world.dart';

/// Objects to use again rather than make and destroy each time, such as
/// shots, enemies or sparks (Unity's `ObjectPool<T>`).
///
/// [get] hands out a hidden one, or makes a new one under [parent];
/// [release] (or the object's own [GameObject.release]) hides it again
/// for the next [get]. A released object starts clean: the routines and
/// motions it and everything below it ran are stopped, and it's back
/// under [parent]. What else it keeps, its fields and the signals it
/// listens to, is the game's to reset: in [get]'s `reset`, or in
/// `onEnable`, which runs on each [get].
///
/// Something that holds on to a pooled object, such as a shot chasing an
/// enemy, keeps an [ObjectHandle] to it instead: the handle goes null once
/// the object is released, even if [get] hands it out again.
///
/// ```dart
/// late final shots = Pool(this, Shot.new);
///
/// void fire() => shots.get((shot) => shot.position = muzzle.position);
/// // in Shot, when it hits or flies out:
/// release();
/// ```
final class Pool<T extends GameObject> {
  /// A pool that adds the objects [make] makes to [parent]. With
  /// [maxSize], a [release] beyond that many hidden objects destroys the
  /// object instead.
  Pool(this.parent, this.make, {this.maxSize}) {
    if (maxSize case final max? when max < 0) throw ArgumentError.value(max, 'maxSize', 'can\'t be negative');
  }

  /// Where new objects are added, and where released ones go back to.
  final GameObject parent;

  /// Makes a new object, as `Shot.new` does.
  final T Function() make;

  /// The most hidden objects the pool keeps, or null for no limit.
  final int? maxSize;

  // The hidden objects, the first [_freeCount] of [_free]: a list that
  // keeps its storage, as `removeLast` doesn't.
  final _free = <T?>[];
  var _freeCount = 0;

  // The objects handed out, the first [_active] of [_out], each knowing
  // its place there (_poolIndex).
  final _out = <T?>[];
  var _active = 0;

  /// The objects handed out and not released.
  int get countActive => _active;

  /// The handed-out object at [index], 0 to one less than [countActive],
  /// to walk them with nothing made: `for (var i = shots.countActive - 1;
  /// i >= 0; i--) shots.activeAt(i).fly()`. Walk from the end to release
  /// as you go: a release moves the last one into its place.
  T activeAt(int index) {
    RangeError.checkValidIndex(index, this, 'index', _active);
    return _out[index]!;
  }

  /// The objects handed out, as a new list: one that stays as it is
  /// whatever is released while it's walked.
  List<T> get active => [for (var i = 0; i < _active; i++) _out[i]!];

  /// The hidden objects waiting for [get].
  int get countInactive => _freeCount;

  /// Every object the pool has that isn't destroyed.
  int get countAll => _active + _freeCount;

  /// A hidden object, or a new one: [reset] gets it first, then it's
  /// shown, so its `onEnable` sees what [reset] did. A new one appears
  /// once it's loaded (see [GameObject.onLoad]).
  T get([void Function(T object)? reset]) {
    final T object;
    if (_freeCount > 0) {
      object = _free[--_freeCount]!;
      _free[_freeCount] = null;
      object._pooled = _PoolState.out;
    } else {
      object = make();
      _adopt(object);
      object._pooled = _PoolState.out;
    }
    _putOut(object);
    reset?.call(object);
    object.active = true;
    return object;
  }

  /// Hides [object] for the next [get]: it and everything below it stop
  /// their routines and motions, it goes back under [parent], and each
  /// [ObjectHandle] to it goes null. Beyond [maxSize] it's destroyed
  /// instead. One destroyed already is left alone. Throws if it isn't this
  /// pool's or was released already.
  void release(T object) {
    if (object._destroyed) return;
    if (!identical(object._pool, this)) throw ArgumentError.value(object, 'object', 'isn\'t from this pool');
    if (object._pooled != _PoolState.out) throw StateError('$object was released already');
    _takeOut(object);
    object._generation++;
    if (maxSize case final max? when _freeCount >= max) {
      object._pooled = _PoolState.none;
      object._pool = null;
      object.destroy();
      return;
    }
    object._pooled = _PoolState.free;
    object.active = false;
    if (object._world case final world?) {
      _stop(world, object);
      if (!identical(object._parent, parent) && !parent._destroyed) object.parent = parent;
    }
    _push(object);
  }

  void _putOut(T object) {
    object._poolIndex = _active;
    if (_active < _out.length) {
      _out[_active] = object;
    } else {
      _out.add(object);
    }
    _active++;
  }

  void _takeOut(GameObject object) {
    final last = _out[--_active]!;
    _out[object._poolIndex] = last;
    last._poolIndex = object._poolIndex;
    _out[_active] = null;
  }

  void _push(T object) {
    if (_freeCount < _free.length) {
      _free[_freeCount] = object;
    } else {
      _free.add(object);
    }
    _freeCount++;
  }

  // Stops what [object] and everything below it run, with no list.
  static void _stop(World world, GameObject object) {
    final components = object._components;
    for (var i = 0; components != null && i < components.length; i++) {
      if (components[i] case final Behaviour b) {
        world._stopRoutines(b._routines);
        world._haltMotions(b._motions);
      }
    }
    world._stopRoutines(object._routines);
    world._haltMotions(object._motions);
    final children = object._children;
    for (var i = 0; children != null && i < children.length; i++) {
      _stop(world, children[i]);
    }
  }

  /// Makes objects until [count] are hidden and waiting, so the first
  /// [get]s needn't make any (and load them) in the middle of play.
  void prewarm(int count) {
    while (_freeCount < count) {
      final object = make();
      _adopt(object);
      object._pooled = _PoolState.free;
      _push(object);
    }
  }

  // Takes [object] in, hidden until it's handed out, so its onEnable
  // comes after the reset.
  void _adopt(T object) {
    if (object._pool != null || object._world != null || object._queuedIn != null) {
      throw StateError('$object is already in use, so this pool can\'t have it: make a new one each time');
    }
    object._pool = this;
    object.active = false;
    parent.add(object);
  }

  /// Destroys the hidden objects; those handed out stay.
  void clear() {
    final free = [for (var i = 0; i < _freeCount; i++) _free[i]!];
    _free.clear();
    _freeCount = 0;
    for (final object in free) {
      object._pooled = _PoolState.none;
      object._pool = null;
      object.destroy();
    }
  }

  // [object], one of this pool's, was destroyed.
  void _lost(GameObject object) {
    switch (object._pooled) {
      case _PoolState.out:
        _takeOut(object);
      case _PoolState.free when parent._destroyed:
        // The parent goes with all the waiting ones: let go of them at
        // once, not one search each.
        for (var i = 0; i < _freeCount; i++) {
          final free = _free[i]!;
          free._pooled = _PoolState.none;
          free._pool = null;
          free._generation++;
        }
        _free.clear();
        _freeCount = 0;
      case _PoolState.free:
        var i = 0;
        while (!identical(_free[i], object)) {
          i++;
        }
        _free.setRange(i, _freeCount - 1, _free, i + 1);
        _free[--_freeCount] = null;
      case _PoolState.none:
    }
    object._pooled = _PoolState.none;
    object._pool = null;
    object._generation++;
  }

  @override
  String toString() => 'Pool<$T>($countActive active, $countInactive inactive)';
}

enum _PoolState { none, out, free }

/// A reference to an object that may be destroyed, or released to its
/// [Pool] and handed out again as something new: [object] is null from
/// then on. For a target, an owner, anything kept across frames.
///
/// ```dart
/// final target = ObjectHandle(enemy);
/// // later:
/// if (target.object case final enemy?) aimAt(enemy.position);
/// ```
final class ObjectHandle<T extends GameObject> {
  /// A handle to [object] as it is now.
  ObjectHandle(T object) : _object = object, _generation = object._generation;

  final T _object;
  final int _generation;

  /// The object, or null once it's destroyed or released to its pool.
  T? get object => isAlive ? _object : null;

  /// Whether [object] is still the one this handle was made for.
  bool get isAlive => !_object._destroyed && _object._generation == _generation;

  @override
  bool operator ==(Object other) =>
      other is ObjectHandle && identical(other._object, _object) && other._generation == _generation;

  @override
  int get hashCode => Object.hash(identityHashCode(_object), _generation);

  @override
  String toString() => isAlive ? 'ObjectHandle($_object)' : 'ObjectHandle(gone)';
}
