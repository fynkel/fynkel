// Behaviours (blueprint §3.2, §3.3, §5.1; ADR-0008, ADR-0011): Dart
// gameplay logic attached to a GameObject, driven by the world at the
// documented points of a host frame with the one lifecycle
//
//   onCreate → onEnable → start → fixedUpdate / afterPhysics / update /
//   lateUpdate → onDisable → onDestroy
//
// Rules the driver keeps: `start` runs once, before the first active
// gameplay callback; a disabled behaviour receives no gameplay callbacks;
// a behaviour is live only while it is enabled and its object is active in
// the hierarchy (`GameObject.setActive`, docs/design/set-active.md) —
// `onCreate` waits for the object's first active tick, `onDisable` runs
// children first and `onEnable` parents first, and `onDestroy` only follows
// an `onCreate`;
// lifecycle transitions (create, enable, disable, destroy) happen only at
// safe boundaries — the start of a callback phase — never in the middle of
// another behaviour's callback; callback order is deterministic: by
// `order`, then attach sequence, whatever the scene or object names are.
//
// Part of world.dart so the driver can reach the world's private state.

part of 'world.dart';

/// Marks a [Behaviour] subclass for generated registration (M2-16 reads it
/// and emits the [BehaviourType]); `id` is the stable, namespaced type id
/// scene components use (`game.PlayerController`).
class FynkelBehaviour {
  const FynkelBehaviour({required this.id, this.version = 1, this.icon, this.category, this.description});

  final String id;
  final int version;

  /// Editor hints (ADR-0018): a Material icon name for Add Component and the
  /// Hierarchy (`'directions_run'`), a category the Add Component menu
  /// groups the type under (`'Gameplay'`), and a description shown there —
  /// the class doc comment when absent. Metadata only.
  final String? icon;
  final String? category;
  final String? description;
}

/// Marks a field as authored data: serialized, shown in the Inspector, set
/// from scene components. Metadata only; the generator reads it and writes
/// every hint into `behaviours.g.json` and the field's schema `x-fynkel`
/// (ADR-0018); the editor never runs this code.
///
/// Display hints beyond the basics: [group] puts the field in a collapsible
/// section, [order] sorts it (lower first; unhinted fields keep declaration
/// order after the hinted ones), [header] is a caption drawn above it,
/// [unit] is shown after the value (`'m'`, `'deg'`, `'s'`, `'m/s'`).
/// [kind] picks the widget when the type alone cannot: `'color'` (a `Vec3`
/// as linear RGB with a picker), `'asset(<kind>)'` (an `AssetRef` whose
/// picker lists only `texture`, `material`, `mesh`, `prefab`, `scene`,
/// `audio`, `clip`, `skin` or `rig` assets), `'entity'` (a `String` holding
/// an entity's name, picked from the scene), `'multiline'` (a `String` as a
/// text area), `'angle'` (a `double` stored in radians, edited in degrees),
/// `'slider'` (a `double`/`int` with [min] and [max] as a slider),
/// `'layerMask'` (an `int` of physics-layer bits). [readOnly] shows the
/// value without editing, [hidden] leaves it out, [showIf] names a sibling
/// exposed `bool` field (`'spinning'`, or `'!spinning'`) the field is shown
/// only when set (unset). Anything else is a generator error naming the
/// field.
class Expose {
  const Expose({
    this.label,
    this.tooltip,
    this.min,
    this.max,
    this.step,
    this.group,
    this.order,
    this.header,
    this.unit,
    this.kind,
    this.readOnly = false,
    this.hidden = false,
    this.showIf,
  });

  final String? label;
  final String? tooltip;
  final num? min;
  final num? max;
  final num? step;
  final String? group;
  final int? order;
  final String? header;
  final String? unit;
  final String? kind;
  final bool readOnly;
  final bool hidden;
  final String? showIf;
}

/// When the editor draws a declarative [Gizmo].
enum GizmoWhen {
  /// While its entity is selected.
  selected,

  /// On every entity carrying the behaviour.
  always,
}

/// The space a declarative [Gizmo]'s value is in.
enum GizmoSpace {
  /// The entity's: moved, turned and scaled with it.
  local,

  /// The world's: a position is absolute, a direction or size keeps its
  /// axes whatever the entity's rotation (it still sits at the entity).
  world,
}

/// Draws an `@Expose` field in the editor's Scene view (ADR-0019 §3): the
/// editor never runs game code, so a behaviour *describes* its gizmos, and
/// the generator writes them into `behaviours.g.json` and the field's
/// schema `x-fynkel` beside the [Expose] hints. Each is also a handle the
/// Inspector's Edit toggle turns on: dragging it sets the field, one undo
/// step. Metadata only.
///
/// - [Gizmo.radius] — a `double`: a wire sphere of that radius about the
///   entity, with a dot per axis to drag.
/// - [Gizmo.box] — a `Vec3`: a wire box of that size (full extents) centred
///   on the entity, a dot per face.
/// - [Gizmo.point] — a `Vec3`: a position (the entity's local space, or the
///   world's), dragged in the view's plane.
/// - [Gizmo.direction] — a `Vec3`: an arrow from the entity along it, its
///   tip dragged.
/// - [Gizmo.cone] — a `double` length along the entity's −Z, with [angle]
///   naming a sibling `double` field holding the half angle in radians: a
///   spot light's shape.
/// - [Gizmo.lineTo] — a `String` naming an entity: a line to it (nothing to
///   drag; pick it in the Inspector).
/// - [Gizmo.path] — a `List<Vec3>`: a polyline through the points, each one
///   dragged in the view's plane.
///
/// [color] is `0xAARRGGBB` (the editor's gizmo colour when null).
class Gizmo {
  const Gizmo.radius({this.color, this.when = GizmoWhen.selected, this.space = GizmoSpace.local})
      : shape = 'radius',
        angle = null;
  const Gizmo.box({this.color, this.when = GizmoWhen.selected, this.space = GizmoSpace.local})
      : shape = 'box',
        angle = null;
  const Gizmo.point({this.color, this.when = GizmoWhen.selected, this.space = GizmoSpace.local})
      : shape = 'point',
        angle = null;
  const Gizmo.direction({this.color, this.when = GizmoWhen.selected, this.space = GizmoSpace.local})
      : shape = 'direction',
        angle = null;
  const Gizmo.cone({required String this.angle, this.color, this.when = GizmoWhen.selected, this.space = GizmoSpace.local})
      : shape = 'cone';
  const Gizmo.lineTo({this.color, this.when = GizmoWhen.selected})
      : shape = 'lineTo',
        space = GizmoSpace.world,
        angle = null;
  const Gizmo.path({this.color, this.when = GizmoWhen.selected, this.space = GizmoSpace.local})
      : shape = 'path',
        angle = null;

  /// `radius`, `box`, `point`, `direction`, `cone`, `lineTo` or `path`.
  final String shape;
  final int? color;
  final GizmoWhen when;
  final GizmoSpace space;

  /// [Gizmo.cone]'s sibling angle field.
  final String? angle;
}

/// What a fixed tick hands to [Behaviour.fixedUpdate].
class FixedUpdateContext {
  const FixedUpdateContext({required this.tick, required this.dt, required this.events, required this.context});

  /// The tick about to be stepped (this callback's commands apply to it).
  final int tick;

  /// Seconds per fixed tick.
  final double dt;

  /// Non-physics events drained since the last callback (spawns, destroys,
  /// parent changes, scene ready, missed deadlines).
  final List<EngineEvent> events;

  final GameContext context;
}

/// What [Behaviour.afterPhysics] receives right after the step: the tick's
/// collision and trigger events; its writes are commands for the next tick.
class AfterPhysicsContext {
  const AfterPhysicsContext({required this.tick, required this.contacts, required this.context});

  /// The tick that just completed.
  final int tick;
  final List<Contact> contacts;
  final GameContext context;
}

/// What a host frame hands to [Behaviour.update] and [Behaviour.lateUpdate].
class UpdateContext {
  const UpdateContext({required this.dt, required this.alpha, required this.tick, required this.context});

  /// Seconds of this host frame.
  final double dt;

  /// Interpolation factor between the last two ticks.
  final double alpha;

  /// Ticks completed so far.
  final int tick;
  final GameContext context;
}

/// Dart gameplay logic attached to one [GameObject]. Subclass, override the
/// callbacks you need, attach with [GameObject.addBehaviour] (or from a
/// scene through the [BehaviourRegistry]).
abstract class Behaviour {
  GameObject? _gameObject;
  _BehaviourSystem? _system;
  int _sequence = 0;
  bool _enabled = true;
  bool _created = false;
  bool _active = false;
  bool _started = false;
  bool _destroyRequested = false;
  bool _destroyed = false;
  BehaviourType? _type;
  String? _authoredComponent;

  /// The registered type this behaviour was built from by
  /// [BehaviourRegistry.instantiate] (a scene's, or
  /// [GameObject.addBehaviourFromData]); null for one the game constructed
  /// itself. The editor's live Play mode reads and writes the fields
  /// through it (ADR-0021 P7).
  BehaviourType? get behaviourType => _type;

  /// The id of the component in the scene (or prefab) document this
  /// behaviour was loaded from; null when no document authored it.
  String? get authoredComponentId => _authoredComponent;

  /// The object this behaviour is attached to; throws before attachment.
  GameObject get gameObject {
    final go = _gameObject;
    if (go == null) throw StateError('$runtimeType is not attached to a GameObject yet');
    return go;
  }

  World get world => gameObject.world;

  /// The world's services (current during callbacks; reachable here always).
  GameContext get context => world.context;

  TransformRef get transform => gameObject.transform;

  /// The object's native components (M3-07a): `get<MeshRenderer>()`,
  /// `set(light.copyWith(intensity: 0))`, `has<Camera>()`,
  /// `remove<SpotLight>()`. See [GameObject.get].
  T? get<T extends WireRecord>() => gameObject.get<T>();
  bool has<T extends WireRecord>() => gameObject.has<T>();
  void set<T extends WireRecord>(T value) => gameObject.set(value);
  void remove<T extends WireRecord>() => gameObject.remove<T>();

  /// The object's rigid body (M3-08): [GameObject.applyForce] and friends.
  void applyForce(Vec3 force) => gameObject.applyForce(force);
  void applyImpulse(Vec3 impulse) => gameObject.applyImpulse(impulse);
  void setLinearVelocity(Vec3 velocity) => gameObject.setLinearVelocity(velocity);
  BodyState? bodyState() => gameObject.bodyState();

  /// The object's character controller (M3-09): [GameObject.move],
  /// [GameObject.jump], [GameObject.characterState].
  void move(Vec3 velocity) => gameObject.move(velocity);
  void jump([double speed = 0]) => gameObject.jump(speed);
  CharacterState? characterState() => gameObject.characterState();

  /// The object's animator (M3-19): `animation.play(clip)`,
  /// `animation.crossfade(clip, 0.2)`, `animation.state`.
  AnimationControl get animation => gameObject.animation;

  /// The object's audio source (M3-11): `audio.play()`, `audio.stop()`,
  /// `audio.volume = 0.5`, `audio.state`. A sound without a source is
  /// `world.audio.play(clip)`.
  AudioControl get audio => gameObject.audio;

  /// Desired state. Setting it queues `onEnable`/`onDisable` for the next
  /// safe boundary; [isActive] says what the driver has applied (enabled
  /// and the object active in the hierarchy).
  bool get enabled => _enabled;
  set enabled(bool value) {
    if (_enabled == value) return;
    _enabled = value;
    _system?._dirty = true;
  }

  /// True between `onEnable` and `onDisable`.
  bool get isActive => _active;

  /// True once `onCreate` ran.
  bool get isCreated => _created;

  /// True once `start` ran.
  bool get hasStarted => _started;

  /// True once `onDestroy` ran (or destruction was requested and the object
  /// is gone).
  bool get isDestroyed => _destroyed;

  /// Callback order among behaviours: lower runs first, ties by attach
  /// order. Override to make an ordering dependency explicit.
  int get order => 0;

  /// Opt in to [afterPhysics].
  bool get wantsAfterPhysics => false;

  /// Removes this behaviour from its object at the next safe boundary
  /// (`onDisable` if active, then `onDestroy`). The object stays.
  void destroy() {
    if (_destroyRequested || _system == null) return;
    _destroyRequested = true;
    _system!._dirty = true;
  }

  /// Another behaviour on the same object.
  T? sibling<T extends Behaviour>() => gameObject.behaviour<T>();

  // ---- the lifecycle (§3.2)

  /// The object and serialized fields exist; references are resolved.
  void onCreate() {}

  /// The behaviour becomes active.
  void onEnable() {}

  /// Once, before the first active gameplay callback.
  void start() {}

  /// Fixed-timestep gameplay: simulation intent for tick `context.tick`.
  void fixedUpdate(FixedUpdateContext context) {}

  /// Optional post-physics hook ([wantsAfterPhysics]).
  void afterPhysics(AfterPhysicsContext context) {}

  /// Contact callbacks (M3-08): right after the tick's step, for every
  /// contact one of whose sides is this behaviour's object. `contact.other(
  /// gameObject.entity)` is the other side; `world.byEntity` finds its
  /// object. Writes here are commands for the next tick.
  void onCollisionEnter(Contact contact) {}
  void onCollisionStay(Contact contact) {}
  void onCollisionExit(Contact contact) {}
  void onTriggerEnter(Contact contact) {}
  void onTriggerExit(Contact contact) {}

  /// Zone callbacks (M3-12): right after the tick's step, for every
  /// `fynkel.TriggerZone` event one of whose sides — the zone or what
  /// entered it — is this behaviour's object. `event.event` is the zone's
  /// name (`"collect"`); `world.byEntity` finds the other side.
  void onZoneEnter(ZoneEvent event) {}
  void onZoneExit(ZoneEvent event) {}

  /// Animation callback (M3-19): right after the tick's step, when this
  /// object's `fynkel.Animator` finished a non-looping clip
  /// (`event.clip`). A looping clip never ends; `stop` does not fire it.
  void onAnimationEnded(AnimationEnded event) {}

  /// Marker callback (M3-19c): right after the tick's step, once per
  /// marker this object's `fynkel.Animator` crossed (`event.name`), loops
  /// and backwards playback included.
  void onAnimationMarker(AnimationMarker event) {}

  /// Audio callback (M3-11): right after the tick's step, when this
  /// object's `fynkel.AudioSource` finished a non-looping clip
  /// (`event.clip`). A looping clip never ends; `stop` and a stolen voice
  /// do not fire it.
  void onPlaybackEnded(PlaybackEnded event) {}

  /// Presentation-rate gameplay logic, once per host frame.
  void update(UpdateContext context) {}

  /// Ordering-sensitive presentation/camera logic, after every `update`.
  void lateUpdate(UpdateContext context) {}

  /// The behaviour becomes inactive.
  void onDisable() {}

  /// Final notification at a safe destruction boundary.
  void onDestroy() {}

  @override
  String toString() => '$runtimeType on ${_gameObject ?? '(detached)'}';
}

/// One authored field of a behaviour type: how to read and write it on an
/// instance, and the Inspector metadata. Generated by M2-16; hand-written
/// in tests.
class BehaviourField {
  const BehaviourField({
    required this.name,
    required this.type,
    required this.get,
    required this.set,
    this.defaultValue,
    this.label,
    this.tooltip,
    this.min,
    this.max,
    this.step,
  });

  /// The scene/JSON field name.
  final String name;

  /// A schema type name: `number`, `integer`, `boolean`, `string`, or a
  /// document `$defs` name such as `assetRef`.
  final String type;
  final Object? Function(Behaviour behaviour) get;

  /// Sets the field from its JSON value; throws [BehaviourDataError] (or a
  /// `TypeError`) when the value does not fit.
  final void Function(Behaviour behaviour, Object? value) set;
  final Object? defaultValue;
  final String? label;
  final String? tooltip;
  final num? min;
  final num? max;
  final num? step;
}

/// A registered behaviour type: id, version, factory and fields — what the
/// generator emits for every `@FynkelBehaviour` class, and what
/// [BehaviourRegistry.instantiate] uses to build one from scene data
/// without reflection.
class BehaviourType {
  const BehaviourType({required this.id, required this.create, this.version = 1, this.fields = const []});

  final String id;
  final int version;
  final Behaviour Function() create;
  final List<BehaviourField> fields;

  BehaviourField? field(String name) {
    for (final f in fields) {
      if (f.name == name) return f;
    }
    return null;
  }
}

/// Bad authored data for a behaviour (unknown type, unknown field, wrong
/// value); the message names the type and field.
class BehaviourDataError extends Error {
  BehaviourDataError(this.typeId, this.message, {this.field});

  final String typeId;
  final String? field;
  final String message;

  @override
  String toString() => 'BehaviourDataError($typeId${field == null ? '' : '.$field'}): $message';
}

/// Behaviour types by id. Registration is explicit (generated code calls
/// [register] for every `@FynkelBehaviour` class); nothing is discovered at
/// runtime.
class BehaviourRegistry {
  final Map<String, BehaviourType> _types = {};

  Iterable<BehaviourType> get types => _types.values;

  /// Registers a type; a second registration of the same id replaces the
  /// first only when [replace] is set, otherwise it is an error.
  void register(BehaviourType type, {bool replace = false}) {
    if (!replace && _types.containsKey(type.id)) {
      throw ArgumentError.value(type.id, 'type', 'a behaviour type with this id is already registered');
    }
    _types[type.id] = type;
  }

  void registerAll(Iterable<BehaviourType> types) => types.forEach(register);

  BehaviourType? lookup(String id) => _types[id];

  bool contains(String id) => _types.containsKey(id);

  /// Builds a behaviour of type [id] with its authored [fields] set (unset
  /// fields keep the class's defaults). Unknown ids and fields, or values
  /// the field rejects, are a [BehaviourDataError]; nothing is attached.
  Behaviour instantiate(String id, {Map<String, Object?> fields = const {}, int? version}) {
    final type = _types[id];
    if (type == null) throw BehaviourDataError(id, 'unknown behaviour type');
    if (version != null && version > type.version) {
      throw BehaviourDataError(id, 'authored version $version is newer than the registered ${type.version}');
    }
    final b = type.create().._type = type;
    for (final entry in fields.entries) {
      final f = type.field(entry.key);
      if (f == null) throw BehaviourDataError(id, 'unknown field', field: entry.key);
      try {
        f.set(b, entry.value);
      } on BehaviourDataError {
        rethrow;
      } catch (e) {
        throw BehaviourDataError(id, 'cannot set from ${entry.value.runtimeType} (${entry.value}): $e', field: entry.key);
      }
    }
    return b;
  }
}

/// A behaviour callback that threw ([World.onBehaviourError]).
class BehaviourError {
  BehaviourError(this.behaviour, this.callback, this.error, this.stack, {required this.tick});

  final Behaviour behaviour;

  /// The callback's name: `update`, `onCollisionEnter`, `start`…
  final String callback;
  final Object error;
  final StackTrace stack;

  /// The world's tick when it threw.
  final int tick;

  /// `Player.update on "Hero" threw: Bad state: no ground` — what the
  /// Console's row says.
  String get message {
    final name = behaviour._gameObject?.name ?? '';
    return '${behaviour.runtimeType}.$callback${name.isEmpty ? '' : ' on "$name"'} threw: $error';
  }

  @override
  String toString() => message;
}

/// The world's behaviour driver.
class _BehaviourSystem {
  _BehaviourSystem(this.world);

  final World world;

  /// Every attached, not yet destroyed behaviour, kept sorted by (order,
  /// sequence).
  final List<Behaviour> _all = [];

  /// Scratch copy for the callback loops (callbacks may attach/destroy);
  /// kept between phases so a steady frame allocates nothing.
  List<Behaviour?> _scratch = List.filled(16, null);
  int _nextSequence = 1;

  /// Set when something needs a settle: an attach, an enabled change, a
  /// destroy request, a spawned or destroyed object.
  bool _dirty = false;

  int get count => _all.length;

  void attach(GameObject go, Behaviour b) {
    if (b._system != null) throw StateError('$b is already attached');
    if (!go._alive) throw StateError('cannot attach $b: $go was destroyed');
    b
      .._gameObject = go
      .._system = this
      .._sequence = _nextSequence++;
    go._behaviours.add(b);
    // Insert sorted by (order, sequence): the order key is read at attach
    // and expected constant afterwards (a getter, so it can depend on
    // constructor arguments).
    var lo = 0, hi = _all.length;
    while (lo < hi) {
      final mid = (lo + hi) >> 1;
      if (_before(_all[mid], b)) {
        lo = mid + 1;
      } else {
        hi = mid;
      }
    }
    _all.insert(lo, b);
    _dirty = true;
  }

  /// Applies pending lifecycle transitions. Called at the start of every
  /// callback phase and after the world resolves spawns and destroys.
  ///
  /// A behaviour is live when it is enabled and its object is active in
  /// the hierarchy (docs/design/set-active.md): `onDisable` runs children
  /// first, then `onCreate`/`onEnable` parents first, each group in
  /// callback order within a depth. `onCreate` waits until the object is
  /// spawned and first active in the hierarchy.
  void settle() {
    // Activity changes show in the world's entity table: a new table (a
    // tick changed the structure, a load, a live edit) is a settle.
    if (_all.isNotEmpty) {
      final t = world._entities;
      if (!identical(t, _table)) {
        _table = t;
        _dirty = true;
      }
    }
    if (!_dirty) return;
    _dirty = false;
    final n = _all.length;
    _copyScratch();
    for (var i = 0; i < n; i++) {
      final b = _scratch[i]!;
      final go = b._gameObject!;
      final gone = !go._alive;
      if (b._destroyRequested || gone) {
        if (b._active) {
          b._active = false;
          _call(b, 'onDisable', b.onDisable);
        }
        b._destroyed = true;
        b._destroyRequested = true;
        _all.remove(b);
        go._behaviours.remove(b);
        if (b._created) _call(b, 'onDestroy', b.onDestroy);
        continue;
      }
      // A pending spawn resolves at the next tick.
      if (go.isPending) {
        if (!b._created) _dirty = true;
        continue;
      }
      final live = go.activeInHierarchy;
      if (!b._created) {
        if (live) _enabling.add(b);
      } else if ((b._enabled && live) != b._active) {
        (b._active ? _disabling : _enabling).add(b);
      }
    }
    _scratch.fillRange(0, n, null);
    if (_disabling.isNotEmpty) {
      if (_disabling.length > 1) _disabling.sort(_childrenFirst);
      for (final b in _disabling) {
        if (!b._active || b._destroyed) continue;
        b._active = false;
        _call(b, 'onDisable', b.onDisable);
      }
      _disabling.clear();
    }
    if (_enabling.isNotEmpty) {
      if (_enabling.length > 1) _enabling.sort(_byDepth);
      for (final b in _enabling) {
        if (b._destroyed) continue;
        if (!b._created) {
          b._created = true;
          _call(b, 'onCreate', b.onCreate);
        }
        if (b._enabled && !b._active && !b._destroyRequested) {
          b._active = true;
          _call(b, 'onEnable', b.onEnable);
        }
      }
      _enabling.clear();
    }
    // A callback may have attached or toggled something: those wait for
    // the next boundary (already flagged dirty by the mutation).
  }

  /// The entity table the last settle saw.
  Object? _table;

  /// This settle's transitions (kept between settles: no allocation).
  final List<Behaviour> _enabling = [], _disabling = [];

  /// Parents before children, then callback order.
  int _byDepth(Behaviour a, Behaviour b) {
    final da = _depth(a), db = _depth(b);
    if (da != db) return da - db;
    return _before(a, b) ? -1 : (_before(b, a) ? 1 : 0);
  }

  /// Children before parents, then callback order.
  int _childrenFirst(Behaviour a, Behaviour b) {
    final da = _depth(a), db = _depth(b);
    if (da != db) return db - da;
    return _before(a, b) ? -1 : (_before(b, a) ? 1 : 0);
  }

  int _depth(Behaviour b) {
    final e = b._gameObject!._entity;
    return e == null ? 0 : world._entities.depthOf(e);
  }

  void _copyScratch() {
    if (_scratch.length < _all.length) _scratch = List.filled(_all.length * 2, null);
    _scratch.setRange(0, _all.length, _all);
  }

  /// One fixed tick's callbacks, right before `step()`.
  void fixedUpdate(int tick, List<EngineEvent> events) {
    settle();
    final ctx = FixedUpdateContext(tick: tick, dt: world.fixedDt, events: events, context: world.context);
    _run(ctx.context, 'fixedUpdate', (b) {
      if (_startIfNeeded(b)) b.fixedUpdate(ctx);
    });
  }

  /// Right after `step()`: the contact callbacks of the objects involved
  /// (in contact order, side `a` then `b`), then `afterPhysics` for the
  /// behaviours that opted in.
  void afterPhysics(int tick, List<Contact> contacts, List<ZoneEvent> zones, List<AnimationEnded> animations,
      List<AnimationMarker> markers, List<PlaybackEnded> playback) {
    if (contacts.isNotEmpty) _dispatchContacts(contacts);
    if (zones.isNotEmpty) _dispatchZones(zones);
    if (animations.isNotEmpty) _dispatchAnimations(animations);
    if (markers.isNotEmpty) _dispatchMarkers(markers);
    if (playback.isNotEmpty) _dispatchPlayback(playback);
    final ctx = AfterPhysicsContext(tick: tick, contacts: contacts, context: world.context);
    _run(ctx.context, 'afterPhysics', (b) {
      if (b.wantsAfterPhysics) b.afterPhysics(ctx);
    });
  }

  void _dispatchContacts(List<Contact> contacts) {
    if (_all.isEmpty) return;
    world.context.run(() {
      for (final c in contacts) {
        _contactTo(world._byEntity[c.a], c);
        if (c.b != c.a) _contactTo(world._byEntity[c.b], c);
      }
    });
  }

  void _dispatchZones(List<ZoneEvent> zones) {
    world.context.run(() {
      for (final z in zones) {
        _zoneTo(world._byEntity[z.zone], z);
        if (z.other != z.zone) _zoneTo(world._byEntity[z.other], z);
        world.zones._dispatch(z);
      }
    });
  }

  void _dispatchAnimations(List<AnimationEnded> ended) {
    if (_all.isEmpty) return;
    world.context.run(() {
      for (final e in ended) {
        final go = world._byEntity[e.entity];
        if (go == null || go._behaviours.isEmpty) continue;
        for (final b in go._behaviours.toList(growable: false)) {
          if (!b._active || b._destroyRequested) continue;
          try {
            b.onAnimationEnded(e);
          } catch (err, st) {
            _threw(b, 'onAnimationEnded', err, st);
          }
        }
      }
    });
  }

  void _dispatchMarkers(List<AnimationMarker> markers) {
    if (_all.isEmpty) return;
    world.context.run(() {
      for (final m in markers) {
        final go = world._byEntity[m.entity];
        if (go == null || go._behaviours.isEmpty) continue;
        for (final b in go._behaviours.toList(growable: false)) {
          if (!b._active || b._destroyRequested) continue;
          try {
            b.onAnimationMarker(m);
          } catch (err, st) {
            _threw(b, 'onAnimationMarker', err, st);
          }
        }
      }
    });
  }

  void _dispatchPlayback(List<PlaybackEnded> ended) {
    if (_all.isEmpty) return;
    world.context.run(() {
      for (final e in ended) {
        final go = world._byEntity[e.entity];
        if (go == null || go._behaviours.isEmpty) continue;
        for (final b in go._behaviours.toList(growable: false)) {
          if (!b._active || b._destroyRequested) continue;
          try {
            b.onPlaybackEnded(e);
          } catch (err, st) {
            _threw(b, 'onPlaybackEnded', err, st);
          }
        }
      }
    });
  }

  void _zoneTo(GameObject? go, ZoneEvent z) {
    if (go == null || go._behaviours.isEmpty) return;
    for (final b in go._behaviours.toList(growable: false)) {
      if (!b._active || b._destroyRequested) continue;
      try {
        switch (z) {
          case ZoneEntered():
            b.onZoneEnter(z);
          case ZoneExited():
            b.onZoneExit(z);
        }
      } catch (e, st) {
        _threw(b, z is ZoneEntered ? 'onZoneEnter' : 'onZoneExit', e, st);
      }
    }
  }

  void _contactTo(GameObject? go, Contact c) {
    if (go == null || go._behaviours.isEmpty) return;
    // A callback may attach or destroy: iterate a copy of the short list.
    for (final b in go._behaviours.toList(growable: false)) {
      if (!b._active || b._destroyRequested) continue;
      try {
        switch (c.kind) {
          case ContactKind.collisionEnter:
            b.onCollisionEnter(c);
          case ContactKind.collisionStay:
            b.onCollisionStay(c);
          case ContactKind.collisionExit:
            b.onCollisionExit(c);
          case ContactKind.triggerEnter:
            b.onTriggerEnter(c);
          case ContactKind.triggerExit:
            b.onTriggerExit(c);
        }
      } catch (e, st) {
        _threw(b, 'on${c.kind.name[0].toUpperCase()}${c.kind.name.substring(1)}', e, st);
      }
    }
  }

  /// Once per host frame: every `update`, then every `lateUpdate`.
  void update(double dt) {
    settle();
    final ctx = UpdateContext(dt: dt, alpha: world.alpha, tick: world.tick, context: world.context);
    _run(ctx.context, 'update', (b) {
      if (_startIfNeeded(b)) b.update(ctx);
    });
    _run(ctx.context, 'lateUpdate', (b) => b.lateUpdate(ctx));
  }

  /// Runs `start` the first time; false when it threw (reported, and the
  /// callback after it skipped this once — Unity's order).
  bool _startIfNeeded(Behaviour b) {
    if (b._started) return true;
    b._started = true;
    return _call(b, 'start', b.start);
  }

  /// Calls [f], one of [b]'s callbacks; a throw goes to
  /// [World.onBehaviourError] and the caller carries on. False when it threw.
  bool _call(Behaviour b, String callback, void Function() f) {
    try {
      f();
      return true;
    } catch (e, st) {
      _threw(b, callback, e, st);
      return false;
    }
  }

  void _threw(Behaviour b, String callback, Object e, StackTrace st) {
    world.onBehaviourError(BehaviourError(b, callback, e, st, tick: world.tick));
  }

  /// Runs [f] over the active behaviours in order, with the world's context
  /// current. Behaviours attached, disabled or destroyed during the pass are
  /// skipped for the rest of it (their transition applies at the next
  /// boundary).
  void _run(GameContext context, String callback, void Function(Behaviour) f) {
    final n = _all.length;
    if (n == 0) return;
    _copyScratch();
    context.run(() {
      for (var i = 0; i < n; i++) {
        final b = _scratch[i]!;
        if (!b._active || b._destroyRequested || !b._gameObject!._alive) continue;
        try {
          f(b);
        } catch (e, st) {
          _threw(b, callback, e, st);
        }
      }
    });
    _scratch.fillRange(0, n, null);
  }

  /// Objects destroyed or resolved by the event drain.
  void objectChanged() => _dirty = true;
}

/// Callback order: `order`, then attach sequence.
bool _before(Behaviour a, Behaviour b) {
  final ao = a.order, bo = b.order;
  return ao != bo ? ao < bo : a._sequence < b._sequence;
}
