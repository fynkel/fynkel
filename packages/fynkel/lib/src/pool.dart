// Pooling (docs/foundation.md 3.2): Unity's `UnityEngine.Pool.ObjectPool<T>`
// and the usual setup of it for GameObjects.

import 'dart:collection' show HashSet;

import 'world.dart' show FynkelObject, GameObject;

/// A stack of reusable objects (Unity's `ObjectPool<T>`): [get] hands out
/// one it holds, or a new one from `create`; [release] gives it back for
/// the next [get]. Instead of making and destroying bullets, sparks or
/// enemies over and over, a game takes them from a pool and returns them.
///
/// `onGet` runs on every object [get] hands out, `onRelease` on every one
/// given back, `onDestroy` on one the pool lets go (past [maxSize], and
/// in [clear]). With `collectionCheck` (the default) releasing an object
/// the pool already holds throws. A [FynkelObject] destroyed while in the
/// pool (a scene load took it, the game destroyed it) is dropped when
/// [get] comes to it, never handed out.
///
/// For GameObjects, [ObjectPool.gameObjects] sets the hooks up.
class ObjectPool<T extends Object> {
  ObjectPool({
    required this._create,
    this._onGet,
    this._onRelease,
    this._onDestroy,
    this.collectionCheck = true,
    this.maxSize = 10000,
  }) {
    if (maxSize <= 0) throw ArgumentError.value(maxSize, 'maxSize', 'must be more than 0');
  }

  /// A pool of GameObjects made by [create] (usually `world.instantiate`
  /// of a prefab, or `world.build` of a spec): an object is switched on
  /// when [get] hands it out and off when it is released — its scripts
  /// get `onEnable` and `onDisable` at the next tick, where a pooled
  /// object resets itself — and destroyed when the pool lets it go.
  /// [onGet] and [onRelease] run after the switch.
  static ObjectPool<GameObject> gameObjects(
    GameObject Function() create, {
    void Function(GameObject object)? onGet,
    void Function(GameObject object)? onRelease,
    bool collectionCheck = true,
    int maxSize = 10000,
  }) =>
      ObjectPool<GameObject>(
        create: create,
        onGet: (o) {
          o.setActive(true);
          onGet?.call(o);
        },
        onRelease: (o) {
          o.setActive(false);
          onRelease?.call(o);
        },
        onDestroy: (o) {
          if (o.isAlive) o.destroy();
        },
        collectionCheck: collectionCheck,
        maxSize: maxSize,
      );

  final T Function() _create;
  final void Function(T)? _onGet;
  final void Function(T)? _onRelease;
  final void Function(T)? _onDestroy;

  /// Whether [release] checks that the object is not in the pool already.
  final bool collectionCheck;

  /// The most objects the pool keeps; one released past it is destroyed.
  final int maxSize;

  final List<T> _stack = [];
  final Set<T> _waiting = HashSet.identity();

  /// Every object the pool made and has not let go, in use or not.
  int get countAll => _countAll;
  int _countAll = 0;

  /// The objects handed out and not released yet.
  int get countActive => _countAll - _stack.length;

  /// The objects waiting in the pool.
  int get countInactive => _stack.length;

  /// An object from the pool, or a new one when it holds none.
  T get() {
    while (_stack.isNotEmpty) {
      final element = _stack.removeLast();
      _waiting.remove(element);
      if (element is FynkelObject && !element.isAlive) {
        _countAll--;
        continue;
      }
      _onGet?.call(element);
      return element;
    }
    final element = _create();
    _countAll++;
    _onGet?.call(element);
    return element;
  }

  /// Makes objects ahead of need — before a level starts, so the first
  /// shots cost nothing — until [count] wait in the pool (no more than
  /// [maxSize]). Each is made by `create` and put away as [release] does
  /// (`onRelease` runs; a GameObject is switched off); `onGet` does not
  /// run. Unity's `defaultCapacity` only reserves room; this makes them.
  void prewarm(int count) {
    while (_stack.length < count && _stack.length < maxSize) {
      final element = _create();
      _countAll++;
      _onRelease?.call(element);
      _stack.add(element);
      _waiting.add(element);
    }
  }

  /// Gives [element] back: the next [get] may hand it out again.
  void release(T element) {
    if (collectionCheck && _waiting.contains(element)) {
      throw StateError('release: this object is already in the pool');
    }
    _onRelease?.call(element);
    if (_stack.length < maxSize) {
      _stack.add(element);
      _waiting.add(element);
    } else {
      _countAll--;
      _onDestroy?.call(element);
    }
  }

  /// Destroys every object waiting in the pool and forgets the ones in
  /// use.
  void clear() {
    final waiting = _stack.toList(growable: false);
    _stack.clear();
    _waiting.clear();
    _countAll = 0;
    for (final e in waiting) {
      _onDestroy?.call(e);
    }
  }
}
