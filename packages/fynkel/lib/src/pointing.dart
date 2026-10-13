part of 'world.dart';

// The world's side of input (docs/stage-4.md §3.6): what's under a finger,
// and the objects' pointer handlers, as Unity's IPointerDownHandler and
// the rest.

enum _Handler { down, click, longPress, dragStart }

final class _Pointing implements _Targets {
  _Pointing(this.world);

  final World world;

  @override
  RayHit? hitTest(double x, double y) {
    final input = world.input;
    if (!(input._width > 0 && input._height > 0)) return null;
    final Camera lens;
    if (input.camera case final own?) {
      if (cameraObjectId(own, world) == -1 || !own.gameObject.activeInHierarchy) return null;
      lens = own;
    } else {
      final w = world.pointer;
      world._sendTransforms();
      check(fy_camera_active(w, scratch.id));
      final camera = scratch.id.value;
      if (camera == 0) return null;
      lens = world._object(camera).get<Camera>()!;
    }
    // What the camera doesn't draw can't be touched through it.
    final drawn = lens.cullingMask;
    final ray = lens.screenPointToRay(ViewPoint(x, y, width: input._width, height: input._height));
    final layers = drawn & input.layers;
    var nearest = world.raycast(ray, layers: layers);
    // What the systems hold that has no mesh: the nearest hit wins.
    final systems = world._systems;
    for (var i = 0; i < systems.length; i++) {
      final system = systems[i];
      if (system._off) continue;
      try {
        final hit = system.pointerHit(ray, layers);
        if (hit != null && (nearest == null || hit.distance < nearest.distance)) nearest = hit;
      } catch (error, stack) {
        world._systemThrew(system, error, stack);
      }
    }
    return nearest;
  }

  @override
  void report(Object error, StackTrace stack) => world._report(null, error, stack);

  @override
  void down(Finger finger) {
    final hit = finger._startedOver = finger.over;
    finger._taker = _bubble(hit?.gameObject, finger, _Handler.down);
  }

  @override
  void up(Finger finger) {
    final taker = finger._taker;
    finger._taker = null;
    if (taker != null) _one(taker, finger, (o) => o.onPointerUp(finger), (b) => b.onPointerUp(finger));
  }

  @override
  void click(Finger finger) => _bubble(finger.over?.gameObject, finger, _Handler.click);

  @override
  void longPress(Finger finger) => _bubble(finger.over?.gameObject, finger, _Handler.longPress);

  @override
  void dragStart(Finger finger) =>
      finger._captor = _bubble(finger._startedOver?.gameObject, finger, _Handler.dragStart);

  @override
  void drag(Finger finger) {
    final captor = finger._captor;
    if (captor != null) _one(captor, finger, (o) => o.onDrag(finger), (b) => b.onDrag(finger));
  }

  @override
  void dragEnd(Finger finger) {
    final captor = finger._captor;
    finger._captor = null;
    if (captor != null) _one(captor, finger, (o) => o.onDragEnd(finger), (b) => b.onDragEnd(finger));
  }

  @override
  void moved(Finger finger) => _overNow(finger, finger.over?.gameObject);

  @override
  void left(Finger finger) => _overNow(finger, null);

  /// Calls [kind]'s handler on [from], then on its behaviours, then on its
  /// parent and its behaviours, and so on up, until one returns true; that
  /// one (the object or the behaviour) is returned.
  Object? _bubble(GameObject? from, Finger finger, _Handler kind) {
    for (var object = from; object != null && !object._destroyed;) {
      final up = object.parent;
      try {
        if (_call(object, null, finger, kind)) return object;
      } catch (error, stack) {
        world._report(object, error, stack);
      }
      final components = object._components;
      for (var j = 0; components != null && j < components.length && !object._destroyed; j++) {
        if (components[j] case final Behaviour b when _listens(b)) {
          try {
            if (_call(null, b, finger, kind)) return b;
          } catch (error, stack) {
            world._report(object, error, stack);
          }
        }
      }
      object = up;
    }
    return null;
  }

  bool _call(GameObject? o, Behaviour? b, Finger f, _Handler kind) => switch (kind) {
    _Handler.down => o?.onPointerDown(f) ?? b!.onPointerDown(f),
    _Handler.click => o?.onClick(f) ?? b!.onClick(f),
    _Handler.longPress => o?.onLongPress(f) ?? b!.onLongPress(f),
    _Handler.dragStart => o?.onDragStart(f) ?? b!.onDragStart(f),
  };

  static bool _listens(Behaviour b) => b._loaded && b._enabledFlag && !b._removed;

  /// Calls the object or behaviour that took a finger's down or drag, if
  /// it's still there.
  void _one(
    Object target,
    Finger finger,
    void Function(GameObject o) onObject,
    void Function(Behaviour b) onBehaviour,
  ) {
    switch (target) {
      case final GameObject o when !o._destroyed:
        try {
          onObject(o);
        } catch (error, stack) {
          world._report(o, error, stack);
        }
      case final Behaviour b when _listens(b) && !(b._owner?._destroyed ?? true):
        try {
          onBehaviour(b);
        } catch (error, stack) {
          world._report(b._owner, error, stack);
        }
    }
  }

  /// The finger is over [object] now (or nothing): each object it left,
  /// from the one it was over up, gets onPointerExit, then each it entered,
  /// from the top down, gets onPointerEnter, as in Unity.
  void _overNow(Finger finger, GameObject? object) {
    final old = finger._chain;
    if (old != null && old.isNotEmpty && identical(old.first, object)) return;
    if (old == null && object == null) return;
    final chain = <GameObject>[];
    for (GameObject? o = object; o != null && !o._destroyed; o = o.parent) {
      chain.add(o);
    }
    finger._chain = chain.isEmpty ? null : chain;
    for (final o in old ?? const <GameObject>[]) {
      if (o._destroyed || chain.contains(o)) continue;
      _each(o, (o) => o.onPointerExit(finger), (b) => b.onPointerExit(finger));
    }
    for (final o in chain.reversed) {
      if (o._destroyed || (old?.contains(o) ?? false)) continue;
      _each(o, (o) => o.onPointerEnter(finger), (b) => b.onPointerEnter(finger));
    }
  }

  /// Calls [onObject] on [object], then [onBehaviour] on its behaviours.
  void _each(GameObject object, void Function(GameObject o) onObject, void Function(Behaviour b) onBehaviour) {
    try {
      onObject(object);
    } catch (error, stack) {
      world._report(object, error, stack);
    }
    final components = object._components;
    for (var j = 0; components != null && j < components.length && !object._destroyed; j++) {
      if (components[j] case final Behaviour b when _listens(b)) {
        try {
          onBehaviour(b);
        } catch (error, stack) {
          world._report(object, error, stack);
        }
      }
    }
  }
}
