part of 'world.dart';

/// An object in a [World]: a [Transform], its components (the engine's
/// [Camera], [Light] and [MeshRenderer], and any number of the game's
/// [Behaviour]s), and the objects added to it.
///
/// Make one, then add it: `world.add(GameObject(...))` or
/// `parent.add(child)`. Until then it only describes itself: its
/// [transform], [name] and components can be read and changed, and objects
/// can be added to it, but anything that needs the world ([parent],
/// [children], [find], [worldTransform]...) throws a [StateError].
///
/// Extend it to give an object its own fields and code: a class is what
/// other engines call a prefab, and each instance is one object. The same
/// instance comes back from [find], [children], [parent] and
/// [World.raycast], so a hit on a character's arm leads to the
/// `Character` with its fields.
///
/// **A subclass is what an object is; a [Behaviour] is what it does.** A
/// `Player`, an `Enemy` or a `Level` is a subclass: its state, its parts,
/// and a constructor that makes one. Code another kind of object could use
/// as well, such as `Health`, `Spin` or `Flash`, is a behaviour, put on
/// each object that needs it. If a subclass's [onUpdate] would make sense
/// on another kind of object, it belongs in a behaviour.
///
/// (Called `GameObject`, not `Object`, because Dart's `Object` is every
/// value's type.)
class GameObject with _Moving {
  /// A new object, not yet in any world, with [transform] (the identity if
  /// not given), [components] (as [addComponent] puts them on) and
  /// [children] (as [add] adds them), so a whole prefab can be one
  /// expression:
  ///
  /// ```dart
  /// GameObject(name: 'enemy', components: [Health(30), Chase()], children: [Body()])
  /// ```
  GameObject({
    String? name,
    Transform? transform,
    Iterable<Component> components = const [],
    Iterable<GameObject> children = const [],
    int layer = 0,
  }) : _described = _Description(name, transform ?? Transform()) {
    _layer = _checkLayer(layer);
    components.forEach(addComponent);
    children.forEach(add);
  }

  /// The object for an engine id the game didn't make, such as a model's
  /// part.
  GameObject._live(World world, int id) : _world = world, _id = id, _appeared = true, _probed = false;

  World? _world;
  int _id = 0;

  // Its layer, kept here: only the game sets one, so it's never read back.
  int _layer = 0;

  // What the object is to be, until it's added.
  _Description? _described;

  // Its place in the hierarchy, kept here and in the engine alike, and
  // changed only by the world (world_objects.dart): the engine draws from
  // its copy, the game reads this one, with no call.
  GameObject? _parent;
  List<GameObject>? _children;

  // The object this one was added to before either was live.
  GameObject? _queuedIn;

  // Destroyed: from [destroy] on. Gone: out of the engine, once its
  // onDestroy calls are done.
  bool _destroyed = false;
  bool _gone = false;

  // The pool it came from, if any (pool.dart), whether it's handed out or
  // waiting, and how many times it was released or destroyed, for
  // [ObjectHandle].
  Pool<GameObject>? _pool;
  _PoolState _pooled = _PoolState.none;
  int _poolIndex = 0;
  int _generation = 0;
  bool _started = false;

  // Whether it had onEnable and not onDisable since: it's shown.
  bool _enabled = false;

  // Its components, in the order they were put on; removed ones stay until
  // the world's next update takes them out. For an object the engine made
  // (a model's part), its engine components are read the first time
  // they're asked for ([_probed]). [_inLoop]: in the world's update order.
  List<Component>? _components;
  bool _probed = true;
  bool _inLoop = false;

  // The transform as the game last set or read it, so reading it makes no
  // call, and whether it waits in [World._unsent]. Null until first read
  // or set, and once the object is gone.
  Transform? _transform;
  bool _unsent = false;

  // The world's transform version when this object's transform or parent
  // was last set (systems.dart).
  int _transformStamp = 0;
  // Its last entry's place in [World._movedLog], or -1.
  int _loggedAt = -1;

  // Its world transform, worked out in Dart from its parents', with what
  // it was worked out from: good while its own transform and its parent's
  // world transform are the same objects (each is made anew when it
  // changes), so one object moving leaves the others' as they are.
  Transform? _worldTransform;
  Transform? _worldLocal;
  Transform? _worldAbove;

  // Loading: how many loads this object waits for (its own, and those of
  // the objects added during it), the object waiting for this one, and
  // whether it's hidden until they're done. [_activeSelf] is the game's
  // own flag, which the engine's follows once the object has appeared.
  int _pending = 0;
  GameObject? _waiter;
  bool _appeared = false;
  bool _hiddenForLoad = false;
  bool _activeSelf = true;
  Completer<void>? _loadedCompleter;
  (Object, StackTrace)? _loadFailure;

  // The assets the object holds a use of, freed when it's destroyed.
  List<_Asset>? _uses;

  // For a model's copy: the model, so the meshes and materials its
  // objects use aren't collected while it's there. Only held, never read.
  // ignore: unused_field
  Model? _model;
  Assets? _assets;

  @override
  GameObject get _self => this;

  @override
  GameObject get _ownerObject => this;

  @override
  bool get _holdsStill => !_enabled;

  @override
  bool get _loading => !_appeared;

  /// The world the object was added to. Throws a [StateError] before it's
  /// added.
  World get world => _world ?? (throw _notAdded('world'));

  /// The engine's id: never 0, and never reused. Throws a [StateError]
  /// before the object is added.
  int get id => _world == null ? throw _notAdded('id') : _id;

  /// Whether the object is live: added to a world, not destroyed, and the
  /// world not disposed.
  bool get isAlive => _world != null && !_destroyed && !_world!.isDisposed && fy_exists(_world!._pointer, _id);

  /// Whether [destroy] was called on this object or one above it.
  bool get isDestroyed => _destroyed;

  /// The engine's world, for what needs this object there; a
  /// [StateError] saying [what] can't be done once it's destroyed.
  Pointer<FyWorld> _engine(String what) {
    if (_gone) throw StateError('$this was destroyed, so $what');
    return world.pointer;
  }

  StateError _notAdded(String what) => StateError(
    '$this has no $what yet: it isn\'t added to a world. Add it first (world.add, or add to an object '
    'that is), and touch the world from its onLoad on.',
  );

  // --- Loading ---

  /// Called once, when the object is added: where it gets its assets
  /// and adds its children. Until it, and everything added during it,
  /// are loaded, the object is hidden: not drawn, not hit, not started or
  /// updated. Then it appears whole, and [loaded] completes.
  ///
  /// Return a future (`Future<void> onLoad() async`) to load something;
  /// an object that loads nothing appears at once, in the same `add`.
  /// A load that throws destroys the object and goes to [World.errors];
  /// the parent's load goes on without it. If the object is destroyed
  /// while it loads, its load finds it gone at its next step (adding to
  /// it and loading for it throw a [StateError]) and is ignored, and
  /// what it loaded is freed.
  @mustCallSuper
  @protected
  FutureOr<void> onLoad() {}

  /// Completes when the object has appeared: it and everything added
  /// during its [onLoad] are loaded. Fails with the load's error if its
  /// load failed, or with a [StateError] if it was destroyed first.
  ///
  /// To keep one level on screen until the next is ready:
  /// `final forest = world.add(Forest()); await forest.loaded; garden.destroy();`.
  Future<void> get loaded {
    if (_appeared) return Future.value();
    if (_loadFailure case (final error, final stack)) return Future.error(error, stack);
    return (_loadedCompleter ??= Completer<void>()).future;
  }

  void _failLoaded(Object error, StackTrace stack) {
    if (_appeared || _loadFailure != null) return;
    _loadFailure = (error, stack);
    _loadedCompleter?.completeError(error, stack);
    _loadedCompleter = null;
  }

  /// The world's assets, loaded for this object: each one it asks for is
  /// held until it's destroyed, and freed then if no one else holds it.
  Assets get assets => _assets ??= Assets._(_world ?? (throw _notAdded('assets')), this);

  // --- The loop ---

  /// What the player did since the last frame: the world's [World.input].
  Input get input => world.input;

  /// The world's [World.debug]: lines drawn over the scene.
  Debug get debug => world.debug;

  /// The world's clock: `time.delta` is the seconds this frame counts for.
  /// See [Time].
  Time get time => world.time;

  /// Starts a routine: runs [body] now, up to its first wait, and returns
  /// the [Routine], which can be awaited for what [body] returns, or
  /// stopped. Its waits ([nextFrame], [wait], [waitUntil], [waitWhile],
  /// [race], [waitAll]) end in the routines step of a later frame, after
  /// every [onUpdate] (docs/stage-4.md §3.2):
  ///
  /// ```dart
  /// startRoutine(() async {
  ///   while (true) {
  ///     await wait(2);
  ///     add(Coin());
  ///   }
  /// });
  /// ```
  ///
  /// It stops when this object is destroyed, and is paused while it's
  /// hidden, though not during its own load. Throws a [StateError] before the object is added.
  Routine<T> startRoutine<T>(Future<T> Function() body) => world._start(this, this, body);

  /// Stops every routine this object started ([startRoutine]), not those
  /// its behaviours did (Unity's `StopAllCoroutines`).
  void stopAllRoutines() => world._stopRoutines(_routines);

  /// Calls [handler] with each value [signal] emits, until this object is
  /// destroyed (with its routines: after its onDisable, before its
  /// onDestroy) or the subscription is cancelled. The usual way to listen:
  /// a destroyed object never hears anything, and nothing holds it. What
  /// [handler] throws goes to [World.errors]. It hears signals while it's
  /// hidden too, as in Unity.
  SignalSubscription<T> on<T>(Signal<T> signal, void Function(T value) handler) {
    if (_destroyed) throw StateError('$this was destroyed, so it can\'t listen');
    return _on(this, signal, handler);
  }

  /// Stops listening with [on], for this object and its behaviours.
  void _stopListening() {
    _listening?.cancelAll();
    for (final c in _components ?? const <Component>[]) {
      if (c is Behaviour) c._listening?.cancelAll();
    }
  }

  /// Called each time the object is shown: when it appears (right after
  /// its load, with everything added during it), and when it, or one
  /// above it, is turned on ([active]) or it's moved out from below a
  /// hidden one. Parents come first, each before its behaviours.
  @mustCallSuper
  @protected
  void onEnable() {}

  /// Called once, in the first [World.update] after the object is added
  /// and shown, before its first [onUpdate]. Its parent's comes first.
  @mustCallSuper
  @protected
  void onStart() {}

  /// Called every fixed step ([Time.fixedDelta], 50 a second by default)
  /// while the object is shown, parents before children, before the
  /// frame's [onUpdate]: for what must not depend on the frame rate, such as
  /// forces on a rigidbody. A frame runs as many steps as its time holds,
  /// maybe none; inside one, `time.delta` is [Time.fixedDelta].
  @mustCallSuper
  @protected
  void onFixedUpdate() {}

  /// Called every [World.update], parents before children, while the
  /// object is shown. Override it to move, animate or think, by
  /// [Time.delta] (`time.delta`); a mixin that overrides it calls
  /// `super.onUpdate()` so the next one runs too.
  @mustCallSuper
  @protected
  void onUpdate() {}

  /// Called every [World.update] after every object's [onUpdate], in the
  /// same order: for what follows where the others went, such as a camera.
  @mustCallSuper
  @protected
  void onLateUpdate() {}

  /// Called each time a shown object is hidden: it or one above it is
  /// turned off, it's moved below a hidden one, or it's destroyed (right
  /// before [onDestroy]). Parents come first, each before its behaviours.
  @mustCallSuper
  @protected
  void onDisable() {}

  /// Called when the object, or one above it, is destroyed: children
  /// before parents, while the object is still there to read (where it
  /// was, what it carried). Adding to it then throws.
  @mustCallSuper
  @protected
  void onDestroy() {}

  /// Called when the app goes away (true: to the background, or hidden)
  /// and comes back (false), while the object is shown, as Unity's
  /// `OnApplicationPause`: to save, as the app may be closed while away.
  /// See [World.applicationPaused].
  @mustCallSuper
  @protected
  void onApplicationPause(bool paused) {}

  // --- Pointers (docs/stage-4.md §3.6) ---
  //
  // As Unity's IPointerDownHandler and the rest. What's under a finger is
  // cast from the camera the view draws from; [Finger.over] is the hit.

  /// A finger went down over this object or something below it that didn't
  /// take it. Return true to take it: this object then gets its
  /// [onPointerUp]. False (the default) passes it to its behaviours, then
  /// to the parent.
  @protected
  bool onPointerDown(Finger finger) => false;

  /// The finger whose [onPointerDown] this object took went up, wherever
  /// it is.
  @protected
  void onPointerUp(Finger finger) {}

  /// A finger came over this object or something below it: the mouse
  /// hovering, or a touch. Each object it enters gets it, from the top
  /// down, as in Unity.
  @protected
  void onPointerEnter(Finger finger) {}

  /// A finger is no longer over this object or anything below it (it
  /// moved off, went up, or left the view).
  @protected
  void onPointerExit(Finger finger) {}

  /// A tap ([Input.onTap]) on this object or something below it that
  /// didn't take it ([Finger.over] is the hit; [Finger.tapCount] is 2 for
  /// a double tap). Return true to take it; false (the default) passes it
  /// to its behaviours, then to the parent.
  @protected
  bool onClick(Finger finger) => false;

  /// A long press ([Input.onLongPress]) on this object or below; bubbles
  /// as [onClick] does.
  @protected
  bool onLongPress(Finger finger) => false;

  /// A drag started from this object or something below it
  /// ([Finger.startedOver]). Return true to take it: this object then gets
  /// the drag's [onDrag] and [onDragEnd], wherever the finger goes. False
  /// (the default) passes it on, as [onClick] does.
  @protected
  bool onDragStart(Finger finger) => false;

  /// The drag this object took moved: [Finger.delta] is the frame's move.
  @protected
  void onDrag(Finger finger) {}

  /// The drag this object took ended.
  @protected
  void onDragEnd(Finger finger) {}

  // --- Adding and destroying ---

  /// Adds [child] below this object and returns it. If this object is
  /// live, [child] is now live too; if not, it becomes live when this one
  /// does.
  ///
  /// Throws a [StateError] if [child] was already added (to move a live
  /// object, set its [parent]) or destroyed, or would end up below itself.
  T add<T extends GameObject>(T child) {
    child._checkAddable();
    if (_destroyed) throw StateError('$this was destroyed, so nothing can be added to it');
    for (GameObject? above = this; above != null; above = above._queuedIn) {
      if (identical(above, child)) throw StateError('$child can\'t be added below itself');
    }
    if (_world != null) {
      _world!._add(child, this);
    } else {
      child._queuedIn = this;
      _described!.children.add(child);
    }
    return child;
  }

  void _checkAddable() {
    if (_destroyed) throw StateError('$this was destroyed; make a new one to add');
    if (_world != null || _queuedIn != null) {
      throw StateError('$this was already added; to move it, set its parent');
    }
  }

  /// Removes the object and everything below it. Each gets its onDisable
  /// and onDestroy calls first, while it can still be read; using any of
  /// them after that throws a [StateError] saying it was destroyed
  /// ([isDestroyed] tells). Destroying an object again does nothing.
  void destroy() {
    if (_destroyed) return;
    final world = _world;
    if (world == null) {
      _queuedIn?._described?.children.remove(this);
      _queuedIn = null;
      _destroyUnadded();
      return;
    }
    if (world.isDisposed || !fy_exists(world._pointer, _id)) {
      _destroyed = true;
      _pool?._lost(this);
      _gone = true;
      return;
    }
    world._destroy(this);
  }

  /// Gives this object back to the [Pool] it came from ([Pool.release]),
  /// or destroys it if it didn't come from one: what a pooled object calls
  /// where it would call [destroy].
  void release() {
    if (_pool case final pool?) return pool.release(this);
    destroy();
  }

  /// Destroys an object that was never added, and what was added to it:
  /// what they own and the assets they hold go too.
  void _destroyUnadded() {
    _destroyed = true;
    _pool?._lost(this);
    _gone = true;
    _stopListening();
    for (final c in _components ?? const <Component>[]) {
      c._removed = true;
    }
    _failLoaded(StateError('$this was destroyed before it was added'), StackTrace.current);
    final described = _described;
    for (final child in described?.children ?? const <GameObject>[]) {
      child._destroyUnadded();
    }
    if (described?.asset case final asset?) {
      asset.world._release(asset, this);
      described!.asset = null;
    }
  }

  // --- Hierarchy ---

  /// The object this one hangs from, or null for a root. Its [transform] is
  /// relative to its parent, so it moves, turns and scales with it.
  ///
  /// Setting it is [setParent] with the object staying where it is in the
  /// world, as Unity's `transform.parent =`.
  GameObject? get parent {
    _engine('its parent can\'t be read');
    return _parent;
  }

  set parent(GameObject? value) => setParent(value);

  // Its parent; before it's added, what it was added to.
  GameObject? get _above => _described != null ? _queuedIn : parent;

  /// Hangs this object from [parent], last among its [children]; null
  /// makes it a root. With [worldPositionStays] (as Unity's `SetParent`)
  /// it stays where it is in the world, turned and sized as it was, so its
  /// [transform] changes; without, its [transform] stays, now counted from
  /// the new parent, so it moves with it. A parent that is this object or
  /// below it throws an [ArgumentError], as does one from another world.
  void setParent(GameObject? parent, {bool worldPositionStays = true}) {
    _engine('its parent can\'t be set');
    if (!worldPositionStays) {
      world._setParent(this, parent);
      return;
    }
    final before = worldTransform;
    world._setParent(this, parent);
    transform = parent == null ? before : parent.worldTransform._under(before, transform.scale);
  }

  /// The objects whose [parent] is this one, in the order they were added.
  /// A new list each time.
  List<GameObject> get children {
    _engine('its children can\'t be read');
    return List.unmodifiable(_children ?? const <GameObject>[]);
  }

  /// Whether this object is [other] or below it (Unity's
  /// `Transform.IsChildOf`, which is also true for itself).
  bool isPartOf(GameObject other) {
    _engine('isPartOf can\'t tell');
    for (GameObject? o = this; o != null; o = o._parent) {
      if (identical(o, other)) return true;
    }
    return false;
  }

  /// The nearest object above this one that is a [T], or null.
  T? ancestor<T extends GameObject>() {
    for (var above = parent; above != null; above = above.parent) {
      if (above is T) return above;
    }
    return null;
  }

  /// Whether this object is on. One that is off hides itself and
  /// everything below it: they aren't drawn, don't light the scene or show
  /// it, and aren't hit by rays. Its children keep their own [active], so
  /// turning it on again shows the ones that were on. See
  /// [activeInHierarchy].
  bool get active {
    if (_described case final d?) return d.active;
    _engine('active can\'t be read');
    return _activeSelf;
  }

  set active(bool value) {
    if (_described case final d?) {
      d.active = value;
      return;
    }
    _engine('active can\'t be set');
    world._setActive(this, value);
  }

  /// The object's layer, 0 to 31 (Unity's layers): a camera draws only the
  /// layers in its [Camera.cullingMask], and a ray ([World.raycast]) or a
  /// finger ([Input.layers]) hits only those in its mask. 0 unless set; a
  /// child doesn't take its parent's.
  int get layer => _layer;

  set layer(int value) {
    _checkLayer(value);
    if (_described == null) check(fy_layer_set(_engine('its layer can\'t be set'), id, value));
    _layer = value;
    if (_world case final world?) {
      world._transformVersion++;
      world._logMoved(this);
    }
  }

  /// Whether this object is shown: it and every object above it [active],
  /// and loaded (see [GameObject.onLoad]).
  bool get activeInHierarchy {
    _engine('activeInHierarchy can\'t be read');
    return world._shown(this);
  }

  /// The object's name, or null. Names needn't be unique; setting '' is
  /// the same as null.
  String? get name {
    if (_described case final d?) return d.name;
    final w = _engine('its name can\'t be read');
    final capacity = scratch.byteCapacity;
    check(fy_name_get(w, id, scratch.bytes(), capacity, scratch.count));
    final length = scratch.count.value;
    if (length == 0) return null;
    if (length > capacity) {
      check(fy_name_get(w, id, scratch.bytes(length), length, scratch.count));
    }
    return const Utf8Decoder().convert(scratch.bytes().asTypedList(length));
  }

  set name(String? value) {
    if (_described case final d?) {
      d.name = value == '' ? null : value;
      return;
    }
    _writeName(value ?? '');
  }

  void _writeName(String value) {
    final length = scratch.utf8(value);
    check(fy_name_set(_engine('its name can\'t be set'), id, scratch.bytes(), length));
  }

  /// The first object below this one named [name], searching depth first
  /// in [children] order, or null. This object itself isn't searched.
  /// Unlike Unity's `Transform.Find`, which looks only at the direct
  /// children and takes `'a/b'` paths, this searches every descendant.
  GameObject? find(String name) {
    final w = _engine('nothing can be found below it');
    final length = scratch.utf8(name);
    check(fy_find(w, id, scratch.bytes(), length, scratch.id));
    final found = scratch.id.value;
    return found == 0 ? null : world._object(found);
  }

  /// Where the object is in the world: its [transform] after its parents',
  /// worked out in Dart with no call into the engine, and kept until
  /// anything moves. Read only; to move it, set [position] and [rotation].
  ///
  /// The position is exact; the rotation is the parents' rotations times
  /// the object's, as Unity's. The scale is approximate when a parent with
  /// an uneven scale has a rotated child: the result is then skewed, which
  /// no position, rotation and scale can describe (Unity's "lossy" scale).
  Transform get worldTransform {
    if (_described != null) {
      final up = _queuedIn;
      return up == null ? transform : up.worldTransform._then(transform);
    }
    _engine('its worldTransform can\'t be read');
    final local = transform;
    final parent = _parent;
    if (parent == null) return local;
    final above = parent.worldTransform;
    if (identical(local, _worldLocal) && identical(above, _worldAbove)) return _worldTransform!;
    _worldLocal = local;
    _worldAbove = above;
    return _worldTransform = above._then(local);
  }

  // --- Components ---

  /// Puts [component] on this object and returns it: a [Behaviour] (any
  /// number), or one of the engine's (at most one [Camera], [Light] and
  /// [MeshRenderer] each). On a live object an engine component is written
  /// at once and a behaviour loads ([Behaviour.onLoad]) and starts the next
  /// frame.
  ///
  /// Throws a [StateError] if [component] is on an object already (each
  /// belongs to one, for its life) or this object was destroyed, an
  /// [ArgumentError] for a second engine component of a kind, and the
  /// engine's error if it refuses the component's values.
  T addComponent<T extends Component>(T component) {
    if (component._owner case final owner?) {
      throw StateError('$component is on $owner already; a component belongs to one object');
    }
    if (_destroyed) throw StateError('$this was destroyed, so nothing can be put on it');
    if (component is! Behaviour) {
      _probe();
      for (final c in _components ?? const <Component>[]) {
        if (!c._removed && c.runtimeType == component.runtimeType) {
          throw ArgumentError('$this has a ${c.runtimeType} already; an object has at most one');
        }
      }
    }
    component._owner = this;
    (_components ??= []).add(component);
    if (_world case final world?) {
      try {
        _write(component);
      } catch (_) {
        _components!.removeLast();
        component._owner = null;
        rethrow;
      }
      world._componentAdded(this, component);
    }
    return component;
  }

  /// Takes [component] off this object. A behaviour gets
  /// [Behaviour.onDisable] if it was enabled, then [Behaviour.onDestroy],
  /// and no calls after; an engine component
  /// is taken out of the engine. It can't be put on again. Throws an
  /// [ArgumentError] if it isn't on this object.
  void removeComponent(Component component) {
    if (!identical(component._owner, this) || component._removed) {
      throw ArgumentError.value(component, 'component', 'is not on $this');
    }
    final world = _world;
    if (world == null) {
      component
        .._removed = true
        .._takenOff = true;
      _components!.remove(component);
      if (component is Behaviour) component._listening?.cancelAll();
      return;
    }
    switch (component) {
      case Behaviour b:
        if (b._enabledNow) {
          b._enabledNow = false;
          world._safely(this, b.onDisable);
        }
        world._stopRoutines(b._routines);
        world._haltMotions(b._motions);
        b._listening?.cancelAll();
        b._ending = true;
        world._safely(this, b.onDestroy);
      case Camera():
        check(fy_camera_remove(world.pointer, id));
      case Light l:
        check(fy_light_remove(world.pointer, id));
        world._countLight(this, l, _LightCount.none);
      case MeshRenderer():
        check(fy_mesh_renderer_remove(world.pointer, id));
    }
    component
      .._removed = true
      .._takenOff = true;
    world._componentRemoved(this, component);
    world._unregister(component);
    world._compact.add(this);
  }

  /// The first component on this object of type [T] (a class, a subclass,
  /// or an interface one implements), or null: `get<Health>()`,
  /// `get<Camera>()`. Unity's `GetComponent`.
  ///
  /// A destroyed object still has the components it had, to read (an
  /// enemy's `Health` after it died): they're [Component.isRemoved], so
  /// they can't be changed. One taken off with [removeComponent] is gone.
  T? get<T extends Object>() {
    _probe();
    for (final c in _components ?? const <Component>[]) {
      if (c is T && !c._takenOff) return c as T;
    }
    return null;
  }

  /// Every component on this object of type [T], in the order they were
  /// put on. A new list each time.
  List<T> getAll<T extends Object>() {
    _probe();
    return [
      for (final c in _components ?? const <Component>[])
        if (c is T && !c._takenOff) c as T,
    ];
  }

  /// Whether this object has a component of type [T].
  bool has<T extends Object>() => get<T>() != null;

  /// The first component of type [T] on this object, or else on the
  /// nearest object above it that has one, or null.
  T? getInParent<T extends Object>() {
    for (GameObject? o = this; o != null; o = o.parent) {
      if (o.get<T>() case final found?) return found;
    }
    return null;
  }

  /// The first component of type [T] on this object, or else below it,
  /// depth first in [children] order, or null.
  T? getInChildren<T extends Object>() {
    if (get<T>() case final found?) return found;
    for (final child in children) {
      if (child.getInChildren<T>() case final found?) return found;
    }
    return null;
  }

  /// Reads the engine components of an object the engine made, the first
  /// time they're asked for, so the same instances come back after.
  void _probe() {
    if (_probed) return;
    _probed = true;
    final world = _world;
    if (world == null || _destroyed || world.isDisposed) return;
    final w = world._pointer;
    bool missing<T>() => !(_components ?? const <Component>[]).any((c) => c is T && !c._removed);
    final found = <Component>[
      if (missing<Camera>() && fy_camera_get(w, _id, scratch.camera) == FyStatus.FY_STATUS_OK)
        Camera(
          fovY: scratch.camera.ref.fov_y.deg,
          near: scratch.camera.ref.near_clip,
          far: scratch.camera.ref.far_clip,
          clearColor: scratch.camera.ref.clear_color.read(),
          orthographic: scratch.camera.ref.orthographic,
          orthographicSize: scratch.camera.ref.size,
          cullingMask: LayerMask._(scratch.camera.ref.culling_mask),
        ),
      if (missing<Light>() && fy_light_get(w, _id, scratch.light) == FyStatus.FY_STATUS_OK) _readLight(),
      if (missing<MeshRenderer>() && fy_mesh_renderer_get(w, _id, scratch.meshRenderer) == FyStatus.FY_STATUS_OK)
        MeshRenderer(
            world._meshes[scratch.meshRenderer.ref.mesh]?.target,
            world._materials[scratch.meshRenderer.ref.material]?.target,
          )
          .._castShadows = scratch.meshRenderer.ref.cast_shadows
          .._receiveShadows = scratch.meshRenderer.ref.receive_shadows,
    ];
    for (final c in found) {
      c._owner = this;
    }
    if (found.isNotEmpty) _components = [...found, ...?_components];
  }

  Light _readLight() {
    final l = scratch.light.ref;
    final kind = switch (l.kind) {
      FY_LIGHT_DIRECTIONAL => LightKind.sun,
      FY_LIGHT_AMBIENT => LightKind.ambient,
      FY_LIGHT_POINT => LightKind.point,
      FY_LIGHT_SPOT => LightKind.spot,
      final kind => throw StateError('the engine returned light kind $kind, which this package:fynkel predates'),
    };
    return Light._(kind, l.color.read(), l.intensity, l.range, l.spot_angle.deg, l.softness, l.shadows);
  }

  void _write(Component component) => switch (component) {
    Camera c => _writeCamera(c),
    Light c => _writeLight(c),
    MeshRenderer c => _writeMeshRenderer(c),
    Behaviour() => null,
  };

  /// Where the object is, how it's turned and how big it is, relative to
  /// its [parent]: [localPosition], [localRotation] and [localScale] in
  /// one. [position] and [rotation] are the world's. Reading it makes no
  /// call into the engine. Setting it
  /// checks the value at once, as the engine would, and sends it with the
  /// frame's other moves in one call when something next reads the
  /// engine's transforms (a view's frame, [World.raycast],
  /// [worldTransform]). It reads back exactly as it was set; the engine
  /// keeps it in 32-bit floats, so what it draws may differ in the last
  /// digits.
  Transform get transform {
    if (_described case final d?) return d.transform;
    final w = _engine('its transform can\'t be read');
    if (_transform case final t?) return t;
    check(fy_transform_get(w, id, scratch.transform));
    final t = scratch.transform.ref;
    return _transform = Transform._(t.position.read(), t.rotation.read(), t.scale.read());
  }

  set transform(Transform value) {
    if (_described case final d?) {
      d.transform = value;
      return;
    }
    final w = _engine('its transform can\'t be set');
    if (_transform == null && !fy_exists(w, _id)) {
      throw StateError('$this was destroyed, or never added');
    }
    _check(value);
    _setTransform(value);
  }

  void _setTransform(Transform value) {
    _transform = value;
    _transformStamp = ++world._transformVersion;
    world._logMoved(this);
    if (_unsent) return;
    _unsent = true;
    world._unsent.add(this);
  }

  /// Throws an [ArgumentError] when the engine would refuse [value]: checked here, so the error comes from the line
  /// that set it, not from the frame that sends it.
  static void _check(Transform value) {
    // In the engine's order: each field's finiteness as 32-bit floats,
    // and the rotation's length after its finiteness.
    final Transform(position: p, rotation: r, scale: s) = value;
    if (!_fits(p)) _refuse('position: NaN or infinity');
    if (!_fits4(r)) _refuse('rotation: NaN or infinity');
    if ((math.sqrt(r.x * r.x + r.y * r.y + r.z * r.z + r.w * r.w) - 1).abs() > 1e-3) {
      _refuse('rotation: rotation is not a unit quaternion (use .normalized)');
    }
    if (!_fits(s)) _refuse('scale: NaN or infinity');
  }

  static final _f32 = Float32List(4);

  // Whether each of a vector's parts is finite as a 32-bit float: the fast
  // test first. They take the vector, not each double, since a double
  // passed to a call the JIT doesn't inline is boxed.
  static bool _fits(Vec3 v) =>
      (v.x.abs() < 3.4e38 && v.y.abs() < 3.4e38 && v.z.abs() < 3.4e38) ||
      ((_f32
            ..[0] = v.x
            ..[1] = v.y
            ..[2] = v.z
            ..[3] = 0)
          .every((x) => x.isFinite));

  static bool _fits4(Quat q) =>
      (q.x.abs() < 3.4e38 && q.y.abs() < 3.4e38 && q.z.abs() < 3.4e38 && q.w.abs() < 3.4e38) ||
      ((_f32
            ..[0] = q.x
            ..[1] = q.y
            ..[2] = q.z
            ..[3] = q.w)
          .every((x) => x.isFinite));

  static Never _refuse(String what) => throw ArgumentError('Transform.$what');

  void _writeCamera(Camera value) {
    final c = scratch.camera.ref;
    c.fov_y = value.fovY.inDegrees;
    c.near_clip = value.near;
    c.far_clip = value.far;
    c.clear_color.write(value.clearColor);
    c.orthographic = value.orthographic;
    c.size = value.orthographicSize;
    c.culling_mask = value.cullingMask.value;
    check(fy_camera_set(_engine('its Camera can\'t be changed'), id, scratch.camera));
  }

  void _writeLight(Light value) {
    final l = scratch.light.ref;
    l.kind = switch (value.kind) {
      LightKind.sun => FY_LIGHT_DIRECTIONAL,
      LightKind.ambient => FY_LIGHT_AMBIENT,
      LightKind.point => FY_LIGHT_POINT,
      LightKind.spot => FY_LIGHT_SPOT,
    };
    l.color.write(value.color);
    l.intensity = value.intensity;
    l.range = value.range;
    l.spot_angle = value.angle.inDegrees;
    l.softness = value.softness;
    l.shadows = value.shadows;
    check(fy_light_set(_engine('its Light can\'t be changed'), id, scratch.light));
    world._countLight(this, value, _LightCount.of(value.kind));
  }

  void _writeMeshRenderer(MeshRenderer value) {
    _engine('its MeshRenderer can\'t be changed');
    final w = world;
    final mesh = value.mesh == null ? 0 : w._meshId(value.mesh!);
    final material = value.material == null ? 0 : w._materialId(value.material!);
    final r = scratch.meshRenderer.ref
      ..mesh = mesh
      ..material = material
      ..own_color = value.color != null
      ..cast_shadows = value.castShadows
      ..receive_shadows = value.receiveShadows;
    r.color.write(value.color ?? Color.white);
    check(fy_mesh_renderer_set(w.pointer, id, scratch.meshRenderer));
  }

  @override
  String toString() => _world == null ? '$runtimeType (not added)' : '$runtimeType#$_id';
}

/// What an object is to be, until it's added: what its constructor and
/// setters said.
final class _Description {
  _Description(this.name, this.transform);

  String? name;
  bool active = true;
  Transform transform;
  final children = <GameObject>[];

  /// For a model's copy ([Model.instantiate]): the engine makes its objects.
  Model? model;

  /// For a model's copy from [Assets.instantiate]: the use it holds of the
  /// model, until it's destroyed.
  _Asset? asset;
}
