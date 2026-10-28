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

  // When a [destroy] or [release] given `after` is due, in the world's game
  // time ([Time.sinceStart]), and which; before it's added, the time after
  // it's added. Whether it's in the world's list of them.
  double? _due;
  bool _dueRelease = false;
  bool _dueListed = false;

  // Whether it had onEnable and not onDisable since: it's shown.
  bool _enabled = false;

  // Its components, in the order they were put on; removed ones stay until
  // the world's next update takes them out. For an object the engine made
  // (a model's part), its engine components are read the first time
  // they're asked for ([_probed]). [_inLoop]: in the world's update order,
  // or waiting to join it ([World._joining]); [_order]: its place there,
  // given when it's added or moved, whether in it or not.
  List<Component>? _components;
  bool _probed = true;
  bool _inLoop = false;
  int _order = 0;

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
  // the objects added during it), the object waiting for this one,
  // whether its loads have been run, and whether it's hidden until
  // they're done. [_activeSelf] is the game's own flag, which the
  // engine's follows once the object has appeared.
  int _pending = 0;
  GameObject? _waiter;
  bool _loadsRun = false;
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
  Animator? _animator;

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
  /// As [World.add], a component the engine refuses on [child] or below it
  /// adds none of them.
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

  /// Adds a new object below this one with [component] on it, and returns
  /// the component, for a field: `_sun = addWith(Light.sun())` where
  /// `add(GameObject(components: [Light.sun()])).get<Light>()!` would. [name], [transform] and [layer] are the new object's, and
  /// [components] go on it after [component], as a camera's rig:
  /// `addWith(VirtualCamera(), components: [Orbit(player)])`.
  T addWith<T extends Component>(
    T component, {
    String? name,
    Transform? transform,
    Iterable<Component> components = const [],
    int layer = 0,
  }) {
    add(GameObject(name: name, transform: transform, components: [component, ...components], layer: layer));
    return component;
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
  ///
  /// With [after], it's destroyed that many seconds of game time later
  /// (Unity's `Destroy(obj, t)`), at the end of that frame's routines,
  /// hidden or not: `world.add(Spark()).destroy(after: 0.5)`. Time slowed
  /// by [Time.scale] or a pause counts as it does for the game. Before it's
  /// added, it counts from when it is. Given twice, the sooner wins.
  void destroy({double after = 0}) {
    if (_destroyed) return;
    if (after != 0) return _later(after, release: false);
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
  /// where it would call [destroy]. Releasing it again before it's handed
  /// out again does nothing, as destroying again does. With [after], it's
  /// released that much later, as [destroy] does: unless it's released
  /// sooner, so its next use isn't cut short.
  void release({double after = 0}) {
    if (after != 0) return _later(after, release: true);
    if (_pool case final pool?) {
      if (_pooled == _PoolState.free) return;
      return pool.release(this);
    }
    destroy();
  }

  void _later(double after, {required bool release}) {
    if (!(after >= 0)) throw ArgumentError.value(after, 'after', 'must be 0 or more');
    if (_destroyed) return;
    final world = _world;
    final due = world == null ? after : world.time._sinceStart + after;
    if (_due case final d? when d <= due) return;
    _due = due;
    _dueRelease = release;
    if (world != null) world._listDue(this);
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
    if (_nameKnown) return _name;
    final capacity = scratch.byteCapacity;
    check(fy_name_get(w, id, scratch.bytes(), capacity, scratch.count));
    final length = scratch.count.value;
    if (length == 0) {
      _nameKnown = true;
      return _name = null;
    }
    if (length > capacity) {
      check(fy_name_get(w, id, scratch.bytes(length), length, scratch.count));
    }
    _nameKnown = true;
    return _name = const Utf8Decoder().convert(scratch.bytes().asTypedList(length));
  }

  // The name as the engine has it, once read or written: only this object
  // changes it.
  String? _name;
  bool _nameKnown = false;

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
    _name = value == '' ? null : value;
    _nameKnown = true;
  }

  /// The first object below this one named [name], searching depth first
  /// in [children] order, or null. This object itself isn't searched.
  /// Unlike Unity's `Transform.Find`, which looks only at the direct
  /// children and takes `'a/b'` paths, this searches every descendant: find
  /// once (in onLoad or onStart) and keep what it gives, rather than each
  /// frame.
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

  /// What plays the model's clips on this copy of it ([Animator]): for an
  /// object made by [Model.instantiate] (or [Assets.instantiate]) from a
  /// model with clips, once it's added; null for any other object, and
  /// for a model's parts.
  Animator? get animator {
    if (_animator case final a?) return a;
    if (_described != null) throw _notAdded('animator');
    _engine('its animator can\'t be used');
    final model = _model;
    if (model == null || model.clips.isEmpty) return null;
    return _animator = world.system(_AnimatorWorld.new)._add(this, model);
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
    if (component case Light(:final _facing?)) {
      component._facing = null;
      rotation = _facing;
    }
    if (_world case final world?) {
      try {
        if (component._enabledFlag) component._put(this);
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
      case _:
        if (component._enabledFlag) component._takeOut(this);
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
    // By index: an iterator would be made each call.
    final components = _components;
    for (var i = 0; components != null && i < components.length; i++) {
      final c = components[i];
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
    // By index, as [children] makes a list.
    final children = _children;
    for (var i = 0; children != null && i < children.length; i++) {
      if (children[i].getInChildren<T>() case final found?) return found;
    }
    return null;
  }

  /// Reads the engine components of an object the engine made, the first
  /// time they're asked for, so the same instances come back after.
  void _probe() {
    if (!_probed) _probeNow();
  }

  // Apart from [_probe], whose every call would otherwise make this one's
  // closure context.
  void _probeNow() {
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
          showSky: scratch.camera.ref.show_sky,
          bloom: scratch.camera.ref.bloom
              ? Bloom(strength: scratch.camera.ref.bloom_strength, threshold: scratch.camera.ref.bloom_threshold)
              : null,
          ambientOcclusion: scratch.camera.ref.ambient_occlusion
              ? AmbientOcclusion(
                  intensity: scratch.camera.ref.ambient_occlusion_intensity,
                  radius: scratch.camera.ref.ambient_occlusion_radius,
                )
              : null,
          pixels: scratch.camera.ref.pixels == 0 ? null : scratch.camera.ref.pixels,
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
    c.show_sky = value.showSky;
    final bloom = value.bloom, ao = value.ambientOcclusion;
    c.bloom = bloom != null;
    c.bloom_strength = bloom?.strength ?? 1;
    c.bloom_threshold = bloom?.threshold ?? 1;
    c.ambient_occlusion = ao != null;
    c.ambient_occlusion_intensity = ao?.intensity ?? 1;
    c.ambient_occlusion_radius = ao?.radius ?? 0.5;
    c.pixels = value.pixels ?? 0;
    check(fy_camera_set(_engine('its Camera can\'t be changed'), id, scratch.camera));
    if (value._effects.isNotEmpty || value._effectsSent) _writeCameraEffects(value);
    if (_world case final world? when (bloom != null && !world._bloomAsked) || (ao != null && !world._aoAsked)) {
      world
        .._bloomAsked |= bloom != null
        .._aoAsked |= ao != null
        .._checkEffects();
    }
  }

  void _writeCameraEffects(Camera value) {
    final w = _engine('its Camera can\'t be changed');
    final effects = value._effects;
    using((arena) {
      final list = arena<FyScreenEffect>(effects.isEmpty ? 1 : effects.length);
      for (final (i, effect) in effects.indexed) {
        final bytes = effect._bytes;
        final e = (list + i).ref;
        e.shader = world._shaderId(effect.shader);
        for (var t = 0; t < 4; t++) {
          e.textures[t] = world._textureOrNone(effect._texture(t));
        }
        e.values_len = bytes?.length ?? 0;
        if (bytes != null) {
          final native = arena<Uint8>(bytes.length);
          native.asTypedList(bytes.length).setAll(0, bytes);
          e.values = native;
        } else {
          e.values = nullptr;
        }
      }
      check(fy_camera_effects_set(w, id, effects.isEmpty ? nullptr : list, effects.length));
    });
    value._effectsSent = effects.isNotEmpty;
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
    final own = value._own;
    for (var i = 0; i < 4; i++) {
      r.own[i] = own == null ? 0 : own[i];
    }
    check(fy_mesh_renderer_set(w.pointer, id, scratch.meshRenderer));
    if (value._more != null || value._moreSent) _writeSubmeshMaterials(value);
  }

  // Apart from [_writeMeshRenderer], whose every call would otherwise make
  // the closure's context, submeshes or not.
  void _writeSubmeshMaterials(MeshRenderer value) {
    final (w, more) = (world, value._more);
    using((arena) {
      final ids = arena<Uint64>(more?.length ?? 0);
      for (var i = 0; i < (more?.length ?? 0); i++) {
        ids[i] = w._materialId(more![i]);
      }
      check(fy_submesh_materials_set(w.pointer, id, more == null ? nullptr : ids, more?.length ?? 0));
    });
    value._moreSent = more != null;
  }

  void _writeTrail(TrailRenderer value) {
    final w = _engine('its TrailRenderer can\'t be changed');
    final views = _trailViews;
    final s = views.settings
      ..time = value.time
      ..min_vertex_distance = value.minVertexDistance
      ..width = value.widthMultiplier
      ..emitting = value.emitting
      ..material = value.material == null ? 0 : world._materialId(value.material!);
    final widths = value._widthTable;
    s.has_width_table = widths != null;
    if (widths != null) {
      final to = views.widthTable;
      for (var i = 0; i < FY_PARTICLE_TABLE; i++) {
        to[i] = widths[i];
      }
    }
    final colors = value._colorTable;
    s.has_color_table = colors != null;
    if (colors != null) _writeColors(views.colorTable, colors);
    check(fy_trail_set(w, id, scratch.trailSettings));
    world.system(_ParticleWorld.new)._trails[id] = value;
  }

  /// [table]'s colours, four numbers each, into [to].
  static void _writeColors(List<FyColor> to, Float32List table) {
    for (var i = 0; i < FY_PARTICLE_TABLE; i++) {
      to[i]
        ..r = table[i * 4]
        ..g = table[i * 4 + 1]
        ..b = table[i * 4 + 2]
        ..a = table[i * 4 + 3];
    }
  }

  static void _writeRange(FyRange to, double min, double max) => to
    ..min = min
    ..max = max;

  /// Writes [value] through views made once ([_ParticleViews]): a system
  /// changed every frame makes nothing here.
  void _writeParticles(ParticleSystem value) {
    final w = _engine('its ParticleSystem can\'t be changed');
    final views = _particleViews;
    final s = views.settings
      ..duration = value.duration
      ..looping = value.looping
      ..prewarm = value.prewarm
      ..play_on_awake = value.playOnAwake
      ..unscaled_time = value.useUnscaledTime
      ..start_delay = value.startDelay
      ..gravity_modifier = value.gravityModifier
      ..space = value.simulationSpace == ParticleSpace.world ? FY_PARTICLE_SPACE_WORLD : FY_PARTICLE_SPACE_LOCAL
      ..simulation_speed = value.simulationSpeed
      ..max_particles = value.maxParticles
      ..stop_action = switch (value.stopAction) {
        StopAction.none => FY_PARTICLE_STOP_NONE,
        StopAction.disable => FY_PARTICLE_STOP_DISABLE,
        StopAction.destroy => FY_PARTICLE_STOP_DESTROY,
        StopAction.callback => FY_PARTICLE_STOP_CALLBACK,
      }
      ..seeded = value.randomSeed != null
      ..seed = value.randomSeed ?? 0
      ..rate_over_time = value.emission.rateOverTime
      ..rate_over_distance = value.emission.rateOverDistance
      ..burst_count = value.emission.bursts.length
      ..material = value.material == null ? 0 : world._materialId(value.material!);
    _writeRange(views.startLifetime, value.startLifetime.min, value.startLifetime.max);
    _writeRange(views.startSpeed, value.startSpeed.min, value.startSpeed.max);
    _writeRange(views.startSize, value.startSize.min, value.startSize.max);
    _writeRange(views.startRotation, value.startRotation.min, value.startRotation.max);
    switch (value.startColor) {
      case final Color c:
        views.startColor[0].write(c);
        views.startColor[1].write(c);
      case ColorRange(:final a, :final b):
        views.startColor[0].write(a);
        views.startColor[1].write(b);
    }
    final bursts = value.emission.bursts;
    for (var i = 0; i < bursts.length; i++) {
      final burst = bursts[i];
      views.bursts[i]
        ..time = burst.time
        ..cycles = burst.cycles
        ..interval = burst.interval
        ..probability = burst.probability;
      _writeRange(views.burstCounts[i], burst.count.min, burst.count.max);
    }
    final shape = value.shape;
    views.shape
      ..kind = switch (shape._kind) {
        _ShapeKind.point => FY_PARTICLE_SHAPE_POINT,
        _ShapeKind.sphere => FY_PARTICLE_SHAPE_SPHERE,
        _ShapeKind.hemisphere => FY_PARTICLE_SHAPE_HEMISPHERE,
        _ShapeKind.cone => FY_PARTICLE_SHAPE_CONE,
        _ShapeKind.box => FY_PARTICLE_SHAPE_BOX,
        _ShapeKind.circle => FY_PARTICLE_SHAPE_CIRCLE,
        _ShapeKind.edge => FY_PARTICLE_SHAPE_EDGE,
      }
      ..radius = shape.radius
      ..radius_thickness = shape.radiusThickness
      ..angle = shape.angle
      ..arc = shape.arc
      ..random_direction = shape.randomDirection;
    views.shapeSize.write(shape.size);
    final sizes = value._sizeTable;
    s.has_size_table = sizes != null;
    if (sizes != null) {
      final to = views.sizeTable;
      for (var i = 0; i < FY_PARTICLE_TABLE; i++) {
        to[i] = sizes[i];
      }
    }
    final colors = value._colorTable;
    s.has_color_table = colors != null;
    if (colors != null) _writeColors(views.colorTable, colors);
    s.has_velocity_over_lifetime = value.velocityOverLifetime != null;
    value.velocityOverLifetime?._write(views.velocity);
    s.has_force_over_lifetime = value.forceOverLifetime != null;
    value.forceOverLifetime?._write(views.force);
    s.has_limit_velocity = value.limitVelocity != null;
    if (value.limitVelocity case final l?) {
      l.limit._write(views.limitCurve);
      l.drag._write(views.drag);
      views.limit.dampen = l.dampen;
    }
    s.has_rotation_over_lifetime = value.rotationOverLifetime != null;
    value.rotationOverLifetime?._write(views.rotation);
    s.has_size_by_speed = value.sizeBySpeed != null;
    if (value.sizeBySpeed case final b?) {
      final to = views.sizeBySpeed;
      for (var i = 0; i < FY_PARTICLE_TABLE; i++) {
        to[i] = b._table[i];
      }
      _writeRange(views.sizeSpeeds, b.minSpeed, b.maxSpeed);
    }
    s.has_color_by_speed = value.colorBySpeed != null;
    if (value.colorBySpeed case final b?) {
      _writeColors(views.colorBySpeed, b._table);
      _writeRange(views.colorSpeeds, b.minSpeed, b.maxSpeed);
    }
    s.has_noise = value.noise != null;
    if (value.noise case final n?) {
      n.strength._write(views.noiseStrength);
      views.noise
        ..frequency = n.frequency
        ..scroll_speed = n.scrollSpeed
        ..octaves = n.octaves;
    }
    s.has_texture_sheet = value.textureSheet != null;
    if (value.textureSheet case final t?) {
      t.frameOverTime._write(views.sheetFrames);
      _writeRange(views.sheetStart, t.startFrame.min, t.startFrame.max);
      views.sheet
        ..columns = t.columns
        ..rows = t.rows
        ..cycles = t.cycles
        ..random_row = t.randomRow;
    }
    final mode = value.renderMode;
    s
      ..render_mode = switch (mode._kind) {
        _RenderKind.billboard => FY_PARTICLE_RENDER_BILLBOARD,
        _RenderKind.stretched => FY_PARTICLE_RENDER_STRETCHED,
        _RenderKind.horizontal => FY_PARTICLE_RENDER_HORIZONTAL,
        _RenderKind.vertical => FY_PARTICLE_RENDER_VERTICAL,
        _RenderKind.mesh => FY_PARTICLE_RENDER_MESH,
      }
      ..length_scale = mode.lengthScale
      ..speed_scale = mode.speedScale
      ..mesh = mode.mesh == null ? 0 : world._meshId(mode.mesh!)
      ..lit = value.lit
      ..softness = value.softness;
    s.has_inherit_velocity = value.inheritVelocity != null;
    if (value.inheritVelocity case final i?) {
      views.inherit.mode = switch (i.mode) {
        InheritVelocityMode.initial => FY_PARTICLE_INHERIT_INITIAL,
        InheritVelocityMode.current => FY_PARTICLE_INHERIT_CURRENT,
      };
      i.curve._write(views.inheritCurve);
    }
    // Planes and sub emitters not in this world yet are left out, and the
    // system written again once they may be. A destroyed plane is left
    // out for good.
    var waiting = false;
    s.has_collision = value.collision != null;
    if (value.collision case final c?) {
      var n = 0;
      final planes = c.planes;
      for (var i = 0; i < planes.length; i++) {
        final plane = planes[i];
        if (plane.isDestroyed) continue;
        if (!identical(plane._world, world)) {
          waiting = true;
          continue;
        }
        views.planes[n++] = plane._id;
      }
      views.collision
        ..plane_count = n
        ..min_kill_speed = c.minKillSpeed
        ..radius_scale = c.radiusScale
        ..send_messages = c.sendCollisionMessages;
      c.dampen._write(views.dampen);
      c.bounce._write(views.bounce);
      c.lifetimeLoss._write(views.lifetimeLoss);
    }
    var subs = 0;
    final subEmitters = value.subEmitters;
    for (var i = 0; i < subEmitters.length; i++) {
      final sub = subEmitters[i];
      final object = sub.system._live;
      if (object == null || !identical(object._world, world) || !sub.system._enabledFlag) {
        waiting = true;
        continue;
      }
      views.subEmitters[subs++]
        ..system = object._id
        ..kind = switch (sub.type) {
          SubEmitterType.birth => FY_PARTICLE_SUB_BIRTH,
          SubEmitterType.collision => FY_PARTICLE_SUB_COLLISION,
          SubEmitterType.death => FY_PARTICLE_SUB_DEATH,
        }
        ..inherit =
            (sub.inherit.contains(SubEmitterInherit.color) ? FY_PARTICLE_INHERIT_COLOR : 0) |
            (sub.inherit.contains(SubEmitterInherit.size) ? FY_PARTICLE_INHERIT_SIZE : 0) |
            (sub.inherit.contains(SubEmitterInherit.rotation) ? FY_PARTICLE_INHERIT_ROTATION : 0)
        ..probability = sub.probability;
    }
    s.sub_emitter_count = subs;
    check(fy_particles_set(w, id, scratch.particleSettings));
    world.system(_ParticleWorld.new)._written(this, value, waiting: waiting);
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
