// Behaviours (blueprint §3.2, §3.3, §5.1; ADR-0008, ADR-0011): Dart
// gameplay logic attached to a GameObject, driven by the world at the
// documented points of a host frame with the one lifecycle
//
//   onCreate → onEnable → start → fixedUpdate / afterPhysics / update /
//   lateUpdate → onDisable → onDestroy
//
// Rules the driver keeps: `start` runs once, before the first active
// gameplay callback; a disabled behaviour receives no gameplay callbacks;
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
  const FynkelBehaviour({required this.id, this.version = 1});

  final String id;
  final int version;
}

/// Marks a field as authored data: serialized, shown in the Inspector, set
/// from scene components. Metadata only; the generator reads it.
class Expose {
  const Expose({this.label, this.tooltip, this.min, this.max, this.step});

  final String? label;
  final String? tooltip;
  final num? min;
  final num? max;
  final num? step;
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

  /// Desired state. Setting it queues `onEnable`/`onDisable` for the next
  /// safe boundary; [isActive] says what the driver has applied.
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
    final b = type.create();
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

  /// Applies pending lifecycle transitions in callback order. Called at the
  /// start of every callback phase and after the world resolves spawns and
  /// destroys.
  void settle() {
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
          b.onDisable();
        }
        b._destroyed = true;
        b._destroyRequested = true;
        _all.remove(b);
        go._behaviours.remove(b);
        if (b._created) b.onDestroy();
        continue;
      }
      if (!b._created) {
        // onCreate waits for the object: a pending spawn resolves at the
        // next tick and its behaviours are created right after.
        if (go.isPending) {
          _dirty = true;
          continue;
        }
        b._created = true;
        b.onCreate();
      }
      if (b._enabled != b._active) {
        b._active = b._enabled;
        if (b._active) {
          b.onEnable();
        } else {
          b.onDisable();
        }
      }
    }
    // A callback may have attached or toggled something: those wait for
    // the next boundary (already flagged dirty by the mutation).
    _scratch.fillRange(0, n, null);
  }

  void _copyScratch() {
    if (_scratch.length < _all.length) _scratch = List.filled(_all.length * 2, null);
    _scratch.setRange(0, _all.length, _all);
  }

  /// One fixed tick's callbacks, right before `step()`.
  void fixedUpdate(int tick, List<EngineEvent> events) {
    settle();
    final ctx = FixedUpdateContext(tick: tick, dt: world.fixedDt, events: events, context: world.context);
    _run(ctx.context, (b) {
      _startIfNeeded(b);
      b.fixedUpdate(ctx);
    });
  }

  /// Right after `step()`: the contact callbacks of the objects involved
  /// (in contact order, side `a` then `b`), then `afterPhysics` for the
  /// behaviours that opted in.
  void afterPhysics(int tick, List<Contact> contacts, List<ZoneEvent> zones) {
    if (contacts.isNotEmpty) _dispatchContacts(contacts);
    if (zones.isNotEmpty) _dispatchZones(zones);
    final ctx = AfterPhysicsContext(tick: tick, contacts: contacts, context: world.context);
    _run(ctx.context, (b) {
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

  static void _zoneTo(GameObject? go, ZoneEvent z) {
    if (go == null || go._behaviours.isEmpty) return;
    for (final b in go._behaviours.toList(growable: false)) {
      if (!b._active || b._destroyRequested) continue;
      switch (z) {
        case ZoneEntered():
          b.onZoneEnter(z);
        case ZoneExited():
          b.onZoneExit(z);
      }
    }
  }

  static void _contactTo(GameObject? go, Contact c) {
    if (go == null || go._behaviours.isEmpty) return;
    // A callback may attach or destroy: iterate a copy of the short list.
    for (final b in go._behaviours.toList(growable: false)) {
      if (!b._active || b._destroyRequested) continue;
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
    }
  }

  /// Once per host frame: every `update`, then every `lateUpdate`.
  void update(double dt) {
    settle();
    final ctx = UpdateContext(dt: dt, alpha: world.alpha, tick: world.tick, context: world.context);
    _run(ctx.context, (b) {
      _startIfNeeded(b);
      b.update(ctx);
    });
    _run(ctx.context, (b) => b.lateUpdate(ctx));
  }

  void _startIfNeeded(Behaviour b) {
    if (!b._started) {
      b._started = true;
      b.start();
    }
  }

  /// Runs [f] over the active behaviours in order, with the world's context
  /// current. Behaviours attached, disabled or destroyed during the pass are
  /// skipped for the rest of it (their transition applies at the next
  /// boundary).
  void _run(GameContext context, void Function(Behaviour) f) {
    final n = _all.length;
    if (n == 0) return;
    _copyScratch();
    context.run(() {
      for (var i = 0; i < n; i++) {
        final b = _scratch[i]!;
        if (b._active && !b._destroyRequested && b._gameObject!._alive) f(b);
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
