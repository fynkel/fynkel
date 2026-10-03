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

/// Marks a plain class as a record (ADR-0027 §5), Unity's `[Serializable]`
/// class: a nested value with `@Expose` fields — a wave's `{enemy, count,
/// delay}` — that a behaviour, a data asset or another record exposes
/// alone or in a `List`. It is stored as a JSON object, has no identity of
/// its own (an assignment copies nothing; a load or a clone makes new
/// instances), and the Inspector draws it as a foldout. It needs an
/// unnamed constructor without required parameters; a key the JSON leaves
/// out keeps the class's default. Its fields hold values only — no
/// `GameObject`, script or data-asset references. Metadata only.
class FynkelRecord {
  const FynkelRecord();
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
/// `audio`, `clip`, `skin` or `rig` assets; a typed asset field such as
/// `MaterialAsset?` implies it), `'multiline'` (a `String` as a
/// text area), `'angle'` (a `double` stored in radians, edited in degrees),
/// `'slider'` (a `double`/`int` with [min] and [max] as a slider),
/// `'layerMask'` (an `int` of physics-layer bits). [readOnly] shows the
/// value without editing, [hidden] leaves it out, [showIf] names a sibling
/// exposed `bool` field (`'spinning'`, or `'!spinning'`) the field is shown
/// only when set (unset). [drawer] names a property drawer an editor
/// extension registers (`editor.namedDrawer('minMax', …)`, ADR-0028): the
/// Inspector draws the field with it (each item, on a list), and without
/// one the default way. Anything else is a generator error naming the
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
    this.drawer,
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
  final String? drawer;
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
/// - [Gizmo.lineTo] — a reference (`GameObject?`, a script,
///   `NativeComponent<T>?`): a line to its object (nothing to drag; pick
///   it in the Inspector).
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
  const Gizmo.cone({
    required String this.angle,
    this.color,
    this.when = GizmoWhen.selected,
    this.space = GizmoSpace.local,
  }) : shape = 'cone';
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
abstract class Behaviour extends Component {
  GameObject? _gameObject;
  _BehaviourSystem? _system;
  int _sequence = 0;
  bool _enabled = true;
  bool _created = false;
  bool _active = false;
  bool _started = false;

  /// Its `start` threw in [_BehaviourSystem._startPending]: the pass's
  /// callback is skipped this once.
  bool _startThrew = false;
  bool _destroyRequested = false;
  bool _destroyed = false;
  // Logical removal published by World.edit; the scheduler retains the binding
  // until it has delivered cleanup callbacks. Legacy requests keep their existing
  // deferred lookup semantics until callback routing is migrated.
  bool _retired = false;
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

  _GameplayAdapter? get _attachmentScope {
    final edit = Zone.current[_gameplayZoneKey] as _GameplayAdapter?;
    if (edit == null || !edit._attachments.containsKey(this)) return null;
    edit.checkOpen();
    return edit;
  }

  _GameplayAdapter? get _lifecycleEdit => _attachmentScope ?? _system?.world._gameplay;

  /// The object this behaviour is attached to; throws before attachment.
  @override
  GameObject get gameObject {
    final go = _attachmentScope?._attachments[this] ?? _gameObject;
    if (go == null) throw StateError('$runtimeType is not attached to a GameObject yet');
    return go;
  }

  World get world => gameObject.world;

  /// The world's services (current during callbacks; reachable here always).
  GameContext get context => world.context;

  @override
  Transform get transform => gameObject.transform;

  /// Attached, not destroyed, and its object alive.
  @override
  bool get isAlive {
    final edit = _lifecycleEdit;
    final go = edit?._attachments[this] ?? _gameObject;
    return !_destroyed && !(edit?.behaviourRemoved(this) ?? _retired) && (go?.isAlive ?? false);
  }

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
  bool get enabled => _lifecycleEdit?.behaviourEnabled(this) ?? _enabled;
  set enabled(bool value) {
    final edit = _lifecycleEdit;
    if (edit != null) return edit.setBehaviourEnabled(this, value);
    if (_system case final system?) {
      return system.world._withGameplay(() => system.world._gameplay!.setBehaviourEnabled(this, value));
    }
    if (_retired) throw StateError('cannot change enabled on a removed behaviour');
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

  /// In [World.edit], removes this behaviour from gameplay lookups immediately;
  /// commit schedules `onDisable` (if active) and `onDestroy` at the next safe
  /// boundary. Outside an edit, an implicit edit commits removal immediately.
  /// The object stays. [isDestroyed] records completed lifecycle destruction.
  @override
  void destroy() {
    final edit = _lifecycleEdit;
    if (edit != null) return edit.destroyBehaviour(this);
    if (_destroyRequested || _system == null) return;
    _system!.world._withGameplay(() => _system!.world._gameplay!.destroyBehaviour(this));
  }

  /// Another behaviour on the same object (one whose `destroy()` was called
  /// is found until the next boundary destroys it, as in Unity).
  T? sibling<T extends Behaviour>() => gameObject.behaviour<T>();

  // ---- coroutines and Invoke (Unity's, docs/unity-parity.md § Tier 2)

  /// Schedules [routine] — a `sync*` generator — for the next variable
  /// scheduler phase after commit, then resumes it once per frame after
  /// every `update`. What it yields says when it goes on: `null` (the
  /// next frame), [WaitForSeconds], [WaitUntil], [WaitWhile],
  /// [WaitForFixedUpdate], another [Coroutine] (until that one ends) or a
  /// nested generator (run in place). It stops at its end, at [Coroutine.stop] /
  /// [stopCoroutine] / [stopAllCoroutines], when this behaviour is
  /// destroyed or its object leaves the active hierarchy, or when it
  /// throws (reported like a callback, [World.onBehaviourError]). Iterator
  /// acquisition and execution always wait until commit and a later frame's
  /// variable scheduler phase.
  /// Abort never runs the routine; the wait clock starts at first execution.
  ///
  /// ```dart
  /// Iterable<Object?> blink() sync* {
  ///   for (var i = 0; i < 3; i++) {
  ///     set(light.copyWith(intensity: 0));
  ///     yield const WaitForSeconds(0.2);
  ///     set(light);
  ///     yield const WaitForSeconds(0.2);
  ///   }
  /// }
  /// ```
  Coroutine startCoroutine(Iterable<Object?> routine) => _schedule(Coroutine._(this, routine, false));

  /// [World.tween] as one of this script's coroutines: it stops with
  /// them (when the script is destroyed or its object leaves the active
  /// hierarchy, or at [stopAllCoroutines]).
  ///
  /// ```dart
  /// tween(0.0, 8.0, 0.5, (v) => set(light.copyWith(intensity: v)), ease: Ease.outCubic);
  /// ```
  Coroutine tween<T extends Object>(
    T from,
    T to,
    double seconds,
    void Function(T value) onValue, {
    Ease ease = Ease.outQuad,
    TimeClock clock = TimeClock.gameplay,
  }) {
    _checkSeconds(seconds);
    final apply = _between(from, to, onValue);
    final system = _lifecycleEdit?.world._behaviours ?? _system;
    if (system == null) {
      (Zone.current[_gameplayZoneKey] as _GameplayAdapter?)?.reject('a tween needs an attached behaviour');
      throw StateError('$runtimeType: a tween needs an attached behaviour');
    }
    return _schedule(
      Coroutine._(this, _tweenRoutine(system, seconds, ease, apply, clock), false)
        .._initialWait = clock == TimeClock.simulation ? const WaitForFixedUpdate() : null,
    );
  }

  /// Stops [coroutine] (it does nothing more).
  void stopCoroutine(Coroutine coroutine) => coroutine.stop();

  /// Stops every coroutine this behaviour started ([invoke]s stay).
  void stopAllCoroutines() {
    final system = _lifecycleEdit?.world._behaviours ?? _system;
    if (system == null) return;
    system.world._withGameplay(() {
      for (final c in _logicalRoutines) {
        if (!c._invoke) c.stop();
      }
    });
  }

  /// Calls [f] once, [seconds] of game time from now (Unity's `Invoke`;
  /// `0` is the next frame). It still runs while this behaviour is
  /// disabled, as Unity's does, and not after it is destroyed.
  Coroutine invoke(void Function() f, double seconds, {TimeClock clock = TimeClock.gameplay}) =>
      invokeRepeating(f, seconds, 0, clock: clock);

  /// Calls [f] after [delay] seconds, then every [rate] seconds (`0`: once).
  Coroutine invokeRepeating(void Function() f, double delay, double rate, {TimeClock clock = TimeClock.gameplay}) {
    _checkSeconds(delay);
    _checkSeconds(rate);
    Iterable<Object?> calls() sync* {
      f();
      while (rate > 0) {
        yield WaitForSeconds(rate, clock: clock);
        f();
      }
    }

    return _schedule(Coroutine._(this, calls(), true).._initialWait = WaitForSeconds(delay, clock: clock));
  }

  /// Cancels [which], or every pending [invoke] of this behaviour.
  void cancelInvoke([Coroutine? which]) {
    if (which != null) return which.stop();
    final system = _lifecycleEdit?.world._behaviours ?? _system;
    if (system == null) return;
    system.world._withGameplay(() {
      for (final c in _logicalRoutines) {
        if (c._invoke) c.stop();
      }
    });
  }

  /// Whether an [invoke] of this behaviour is pending.
  bool get isInvoking => _logicalRoutines.any((c) => c._invoke && c.isRunning);

  final List<Coroutine> _routines = [];
  Iterable<Coroutine> get _logicalRoutines {
    final edit = _lifecycleEdit;
    return edit == null ? _routines : [..._routines, ...edit.routinesFor(this)];
  }

  Coroutine _schedule(Coroutine c) {
    final edit = _lifecycleEdit ?? Zone.current[_gameplayZoneKey] as _GameplayAdapter?;
    if (edit != null) return edit.schedule(c);
    final system = _system;
    if (system == null || _destroyRequested || !_gameObject!._alive || system.world.isDisposed) {
      throw StateError('$runtimeType: a coroutine needs an attached behaviour that is not destroyed');
    }
    return system.world._withGameplay(() => system.world._gameplay!.schedule(c));
  }

  void _stopRoutines({required bool invokes}) {
    for (final c in _routines) {
      if (invokes || !c._invoke) c._done = true;
    }
  }

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
  /// contact one of whose sides is this behaviour's object — its collider,
  /// or the body a child's collider joined (ADR-0034). `contact.other(
  /// gameObject.entity)` is the other side's collider; `world.byEntity`
  /// finds its object. Writes here are commands for the next tick.
  void onCollisionEnter(Contact contact) {}
  void onCollisionStay(Contact contact) {}
  void onCollisionExit(Contact contact) {}
  void onTriggerEnter(Contact contact) {}
  void onTriggerExit(Contact contact) {}

  /// Every tick a collider stays inside this object's sensor, or this
  /// object stays inside another's (Unity's `OnTriggerStay`), after the
  /// tick of the enter. No contact geometry.
  void onTriggerStay(Contact contact) {}

  /// Every tick this object's `CharacterController` touched a collider
  /// while moving — walls, floors, ceilings, static and kinematic
  /// geometry too (Unity's `OnControllerColliderHit`): `hit.b` the
  /// collider, `hit.bodyB` its body, `hit.point`, `hit.normalFor(hit.b)`
  /// the surface normal. Once per collider per tick; the character's
  /// object only. `characterState.collisionFlags` says where it touched.
  void onControllerColliderHit(Contact hit) {}

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

  /// Animator Controller callbacks (ADR-0032; Unity's
  /// `StateMachineBehaviour.OnStateEnter`/`OnStateExit`, kept on the
  /// object's own scripts): right after the tick's step, when a layer of
  /// this object's `fynkel.Animator` controller entered a state (at the
  /// start of the crossfade into it) or stopped playing one (at the end
  /// of the crossfade out). On a jump the exit comes first.
  void onAnimatorStateEntered(AnimatorStateEntered event) {}
  void onAnimatorStateExited(AnimatorStateExited event) {}

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
    this.target,
    this.dataType,
    this.component,
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

  /// A reference field's object as it is (the [GameObject], the
  /// [Behaviour], or null), which [get] answers as JSON; what
  /// [GameObject.clone] copies. Null for any other field, and in code
  /// generated before it existed.
  final Object? Function(Behaviour behaviour)? target;

  /// The type id a data-typed field (or list of them) holds (ADR-0027 §4):
  /// its JSON is `{"asset": id}` and [set] takes the shared [DataAsset]
  /// instance(s); [target] answers them as they are. Null otherwise.
  final String? dataType;

  /// The built-in component a `NativeComponent<T>` field holds (type
  /// `component`, ADR-0036 §2): its schema name (`fynkel.RigidBody`).
  /// Null otherwise.
  final String? component;
  final Object? defaultValue;
  final String? label;
  final String? tooltip;
  final num? min;
  final num? max;
  final num? step;

  /// A GameObject, script or built-in component reference (type
  /// `gameObject` / `behaviour` / `component`): its JSON is `{"entity":
  /// id}` (`, "component": id` for a script), and [set] takes the resolved
  /// object ([GameObject] / the [Behaviour], or null; a component field
  /// takes the [GameObject] or the [NativeComponent]) — the world resolves
  /// it once every behaviour of the load is attached, so a reference may
  /// name one attached after it.
  bool get isReference => type == 'gameObject' || type == 'behaviour' || type == 'component';

  /// Whether it holds data assets ([dataType]).
  bool get holdsData => dataType != null;
}

/// The JSON a reference field's `get` answers for [target] (a
/// [GameObject] or a [Behaviour], or null): the object's live id — what
/// [World.liveIdOf] answers, else its authoring id, else
/// `runtime:<handle>` — and a script's authored component id. Generated
/// code calls it.
Map<String, Object?>? referenceJson(Object? target) {
  if (target != null && _writingReference != null) return _writingReference!(target);
  final GameObject go;
  String? component;
  switch (target) {
    case null:
      return null;
    case GameObject g:
      go = g;
    case Behaviour b:
      go = b.gameObject;
      component = b.authoredComponentId;
    case NativeComponent c:
      // One built-in component of a kind per object: the object names it.
      go = c.gameObject;
    default:
      throw ArgumentError.value(target, 'target', 'not a GameObject, a Behaviour or a NativeComponent');
  }
  if (go.isPending || !go.isAlive) return null;
  final id = go.world.liveIdOf?.call(go.entity) ?? go.authoringId ?? 'runtime:${go.entity}';
  return {'entity': id, 'component': ?component};
}

Map<String, Object?>? Function(Object target)? _writingReference;

/// For `package:fynkel/authoring.dart`: runs [body] with every
/// [referenceJson] answered by [resolve] — the ids of the document being
/// written, for a spec's object or a script in it (ADR-0036 §4).
T writingReferences<T>(Map<String, Object?> Function(Object target) resolve, T Function() body) {
  final before = _writingReference;
  _writingReference = resolve;
  try {
    return body();
  } finally {
    _writingReference = before;
  }
}

/// The spec a [GameObject] stands for when it is an [ObjectSpec.object]
/// not built yet; null for any other object.
ObjectSpec? specOfObject(GameObject go) => go._spec;

/// What a `NativeComponent<T>?` field's generated `set` stores: null, the
/// handle itself, or the component [T] of a [GameObject] (what a load
/// resolves). Throws [ArgumentError] when the object has no [T], so the
/// load reports the field as None.
NativeComponent<T>? nativeComponentOf<T extends WireRecord>(Object? value) => switch (value) {
  null => null,
  final NativeComponent<T> c => c,
  final GameObject go => go.component<T>() ?? (throw ArgumentError('"${go.name}" has no ${componentSchemaName(T)}')),
  _ => throw ArgumentError.value(value, 'value', 'not a GameObject or a NativeComponent<$T>'),
};

/// The schema name of the native component class [t] (`fynkel.RigidBody`).
String componentSchemaName(Type t) {
  final id = gen.componentIds[t];
  return id == null ? '$t' : gen.componentCodecs[id]!.name;
}

/// A registered behaviour type: id, version, factory and fields — what the
/// generator emits for every `@FynkelBehaviour` class, and what
/// [BehaviourRegistry.instantiate] uses to build one from scene data
/// without reflection.
class BehaviourType {
  const BehaviourType({
    required this.id,
    required this.create,
    this.version = 1,
    this.fields = const [],
    this.callbacks,
    this.dartType,
  });

  final String id;
  final int version;
  final Behaviour Function() create;
  final List<BehaviourField> fields;

  /// Null means manually registered: conservatively dispatch every phase.
  final Set<String>? callbacks;
  final Type? dartType;

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
  int _revision = 0;
  int get revision => _revision;
  final Map<String, BehaviourType> _types = {};
  final Map<String, DataType> _dataTypes = {};

  Iterable<BehaviourType> get types => _types.values;

  /// The `@FynkelDataAsset` types (ADR-0027), which the generated
  /// `registerBehaviours` registers with the behaviours.
  Iterable<DataType> get dataTypes => _dataTypes.values;

  /// Registers a data type; like [register], a second registration of an
  /// id is an error unless [replace] is set.
  void registerData(DataType type, {bool replace = false}) {
    if (!replace && _dataTypes.containsKey(type.id)) {
      throw ArgumentError.value(type.id, 'type', 'a data type with this id is already registered');
    }
    _dataTypes[type.id] = type;
  }

  DataType? lookupData(String id) => _dataTypes[id];

  /// The registered data type of [instance]'s class, or null.
  DataType? dataTypeOf(DataAsset instance) {
    final known = instance._dataType;
    if (known != null) return known;
    for (final t in _dataTypes.values) {
      if (t.create().runtimeType == instance.runtimeType) return instance._dataType = t;
    }
    return null;
  }

  /// Builds a data asset of type [id] with its authored [fields] set, as
  /// [instantiate] does for a behaviour. Data-typed fields are left for
  /// the caller to resolve ([DataField.holdsData]).
  DataAsset instantiateData(String id, {Map<String, Object?> fields = const {}, int? version}) {
    final type = _dataTypes[id];
    if (type == null) throw DataAssetError(id, 'unknown data type');
    if (version != null && version > type.version) {
      throw DataAssetError(id, 'authored version $version is newer than the registered ${type.version}');
    }
    final d = type.create();
    final edit = Zone.current[_gameplayZoneKey] as _GameplayAdapter?;
    if (edit != null) {
      edit.checkOpen();
      if (d._asset != null || d._destroyed || !edit._createdData.add(d)) {
        edit.reject('data factory must return a fresh unbound instance');
      }
    }
    d._dataType = type;
    for (final entry in fields.entries) {
      final f = type.field(entry.key);
      if (f == null) throw DataAssetError(id, 'unknown field', field: entry.key);
      if (f.holdsData) continue;
      try {
        f.set(d, entry.value);
      } on DataAssetError {
        rethrow;
      } catch (e) {
        throw DataAssetError(id, 'cannot set from ${entry.value.runtimeType} (${entry.value}): $e', field: entry.key);
      }
    }
    return d;
  }

  /// Registers a type; a second registration of the same id replaces the
  /// first only when [replace] is set, otherwise it is an error.
  void register(BehaviourType type, {bool replace = false}) {
    if (!replace && _types.containsKey(type.id)) {
      throw ArgumentError.value(type.id, 'type', 'a behaviour type with this id is already registered');
    }
    _types[type.id] = type;
    _revision++;
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
    final edit = Zone.current[_gameplayZoneKey] as _GameplayAdapter?;
    if (edit != null) {
      edit.checkOpen();
      if (b._system != null || edit._attachments.containsKey(b)) {
        edit.reject('behaviour factory returned an already attached instance');
      }
    }
    b._type = type;
    for (final entry in fields.entries) {
      final f = type.field(entry.key);
      if (f == null) throw BehaviourDataError(id, 'unknown field', field: entry.key);
      // A reference names an object of the load: the world sets it once
      // they are all there (World._attachBehaviours); a data asset is
      // resolved by the world too.
      if (f.isReference || f.holdsData) continue;
      try {
        f.set(b, entry.value);
      } on BehaviourDataError {
        rethrow;
      } catch (e) {
        throw BehaviourDataError(
          id,
          'cannot set from ${entry.value.runtimeType} (${entry.value}): $e',
          field: entry.key,
        );
      }
    }
    return b;
  }
}

/// A running [Behaviour.startCoroutine] or [Behaviour.invoke].
final class Coroutine {
  Coroutine._(this._owner, this._routine, this._invoke, [this._target]);
  Iterable<Object?>? _routine;
  _BehaviourSystem? _system;
  Object? _initialWait;

  /// The behaviour that started it; null for a tween started on an object
  /// or on the world.
  final Behaviour? _owner;

  /// The object a tween moves: it ends when that object is destroyed.
  final GameObject? _target;
  final List<Iterator<Object?>> _stack = [];
  final bool _invoke;
  bool _done = false;

  /// What the last `yield` waits for, and when it was yielded.
  Object? _wait;
  int _frame = 0, _tick = 0;
  double _until = 0;

  /// False once it ended, was stopped, threw, or its scheduling edit aborted.
  /// Inside an edit this includes staged cancellation.
  bool get isRunning {
    final edit = _system?.world._gameplay;
    return !_done && !(edit?.routineStopped(this) ?? false);
  }

  /// Stops it: it does nothing more. Inside [World.edit], cancellation is
  /// visible immediately in that edit and applied only if the edit commits.
  void stop() {
    final edit = _system?.world._gameplay;
    if (edit != null) return edit.stopRoutine(this);
    if (_system case final system?) {
      return system.world._withGameplay(() => system.world._gameplay!.stopRoutine(this));
    }
    _done = true;
  }

  /// Completes once it ended, was stopped, or threw (at the end of that
  /// frame's pass) — `await tween.done` in async code; a coroutine yields
  /// the [Coroutine] itself instead.
  Future<void> get done {
    final completer = _completer ??= Completer<void>();
    if (_completed && !completer.isCompleted) completer.complete();
    return completer.future;
  }

  bool _completed = false;
  void _finish() {
    _done = true;
    _completed = true;
    _routine = null;
    _stack.clear();
    if (_completer case final c? when !c.isCompleted) c.complete();
  }

  Completer<void>? _completer;
}

/// `yield WaitForSeconds(s)`: go on once [seconds] of game time passed.
final class WaitForSeconds {
  const WaitForSeconds(this.seconds, {this.clock = TimeClock.gameplay});
  final TimeClock clock;
  final double seconds;
}

/// `yield WaitUntil(() => done)`: go on the first frame [condition] holds.
final class WaitUntil {
  const WaitUntil(this.condition);
  final bool Function() condition;
}

/// `yield WaitWhile(() => busy)`: go on the first frame [condition] fails.
final class WaitWhile {
  const WaitWhile(this.condition);
  final bool Function() condition;
}

/// `yield const WaitForFixedUpdate()`: go on after the next fixed tick's
/// physics step (after `afterPhysics`).
final class WaitForFixedUpdate {
  const WaitForFixedUpdate();
}

/// A behaviour callback that threw ([World.onBehaviourError]).
class BehaviourError {
  BehaviourError(this.behaviour, this.callback, this.error, this.stack, {required this.tick, GameObject? gameObject})
    : _object = gameObject;

  final GameObject? _object;

  /// Null for a tween started on an object or on the world.
  final Behaviour? behaviour;

  /// The callback's name: `update`, `onCollisionEnter`, `start`…
  final String callback;
  final Object error;
  final StackTrace stack;

  /// The world's tick when it threw.
  final int tick;

  /// `Player.update on "Hero" threw: Bad state: no ground` — what the
  /// Console's row says.
  String get message {
    final name = (behaviour?._gameObject ?? _object)?.name ?? '';
    return '${behaviour?.runtimeType ?? 'World'}.$callback${name.isEmpty ? '' : ' on "$name"'} threw: $error';
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

  /// Every running coroutine and invoke, in start order.
  final List<Coroutine> _routines = [];

  double _seconds(TimeClock clock) => world.context.time.seconds(clock);

  /// Scratch copy for the callback loops (callbacks may attach/destroy);
  /// kept between phases so a steady frame allocates nothing.
  List<Behaviour?> _scratch = List.filled(16, null);
  int _nextSequence = 1;

  /// Set when something needs a settle: an attach, an enabled change, a
  /// destroy request, a spawned or destroyed object.
  bool _dirty = false;

  int get count => _all.length;

  void attach(GameObject go, Behaviour b) {
    world._rejectDuringEdit('behaviour attach');
    if (b._system != null) throw StateError('$b is already attached');
    if (!go._alive) throw StateError('cannot attach $b: $go was destroyed');
    b
      .._gameObject = go
      .._system = this
      .._sequence = _nextSequence++;
    go._behaviours.add(b);
    // Scripted objects stay in the snapshot: the physics reports stay
    // events and character hits for what Dart observes (foundation 4.4).
    go.world._observe(go);
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
  bool _entryReady({bool force = false}) {
    if (world._callbackEntryBlocked || world.isDisposed) return false;
    if (!force && !_all.any((b) => b._created)) return true;
    world._callbackPreflight = true;
    try {
      world._callbackGameplay(() {});
      return true;
    } on _GameplayCallbackEntrySkipped {
      return false;
    } finally {
      world._callbackPreflight = false;
    }
  }

  void settle() {
    if (world._callbackEntryBlocked || world.isDisposed) return;
    if (_all.any((b) => !b._gameObject!.isPending) && !_entryReady(force: true)) return;
    if (!_dirty) return;
    _dirty = false;
    final n = _all.length;
    _copyScratch();
    for (var i = 0; i < n; i++) {
      if (!_entryReady()) {
        _dirty = true;
        _scratch.fillRange(0, n, null);
        return;
      }
      final b = _scratch[i]!;
      final go = b._gameObject!;
      final gone = !go._alive;
      if (b._destroyRequested || gone) {
        if (b._active) {
          b._active = false;
          _call(b, 'onDisable', b.onDisable);
          if (world.isDisposed) return;
        }
        b._destroyed = true;
        b._destroyRequested = true;
        _deaths = true;
        b._stopRoutines(invokes: true);
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
      // A disabled script still owns routines; hierarchy deactivation cancels
      // them even when there is no active-to-inactive lifecycle transition.
      if (!live) b._stopRoutines(invokes: false);
      if (!b._created) {
        if (live) _enabling.add(b);
      } else if ((b._enabled && live) != b._active) {
        (b._active ? _disabling : _enabling).add(b);
      }
    }
    _scratch.fillRange(0, n, null);
    if (_deaths) {
      _deaths = false;
      _clearDeadReferences();
    }
    if (_disabling.isNotEmpty) {
      if (_disabling.length > 1) _disabling.sort(_childrenFirst);
      for (final b in _disabling) {
        if (!_entryReady()) {
          _dirty = true;
          return;
        }
        if (!b._active || b._destroyed) continue;
        b._active = false;
        // Unity: an object leaving the active hierarchy stops its
        // coroutines; a disabled behaviour keeps them.
        if (!b._gameObject!.activeInHierarchy) b._stopRoutines(invokes: false);
        _call(b, 'onDisable', b.onDisable);
        if (world.isDisposed) return;
      }
      _disabling.clear();
    }
    if (_enabling.isNotEmpty) {
      if (_enabling.length > 1) _enabling.sort(_byDepth);
      for (final b in _enabling) {
        if (!_entryReady()) {
          _dirty = true;
          return;
        }
        if (b._destroyed) continue;
        if (!b._created) {
          b._created = true;
          _call(b, 'onCreate', b.onCreate);
          if (world.isDisposed) return;
        }
        if (b._enabled && !b._active && !b._destroyRequested) {
          b._active = true;
          _call(b, 'onEnable', b.onEnable);
          if (world.isDisposed) return;
        }
      }
      _enabling.clear();
    }
    // A callback may have attached or toggled something: those wait for
    // the next boundary (already flagged dirty by the mutation).
  }

  /// The entity table the last settle saw.

  /// Set when an object or a script died since the last settle: the
  /// settle then clears the script fields that point at it.
  bool _deaths = false;

  /// An object known to the SDK was destroyed.
  void objectDied() {
    _deaths = true;
    _dirty = true;
  }

  /// Every script's registered reference fields, by class; rebuilt when
  /// the registry gains types.
  final Map<Type, List<BehaviourField>> _referenceFields = {};
  int _referenceFieldsCount = -1;

  /// Unity's `== null` after `Destroy` (ADR-0036 §5): a script's
  /// GameObject or script field whose target died reads null from now
  /// on. Visits only the scripts whose class has reference fields.
  void _clearDeadReferences() {
    if (_referenceFieldsCount != world.registry.revision) {
      _referenceFields.clear();
      _referenceFieldsCount = world.registry.revision;
    }
    for (final b in _all) {
      if (b._destroyed) continue;
      final fields = _referenceFields[b.runtimeType] ??= _referenceFieldsOf(b);
      for (final f in fields) {
        final target = f.target!(b);
        if (target is FynkelObject && !target.isAlive) f.set(b, null);
      }
    }
  }

  List<BehaviourField> _referenceFieldsOf(Behaviour b) {
    final type = b._type == null ? world._registeredTypeOf(b) : world.registry.lookup(b._type!.id) ?? b._type;
    if (type == null) return const [];
    return [
      for (final f in type.fields)
        if (f.isReference && f.target != null) f,
    ];
  }

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
    final go = b._gameObject!;
    if (!go._alive || go._entity == null) return 0;
    return world._withGameplay(() {
      final edit = world._gameplay!;
      var depth = 0;
      var parent = edit.native.parent(edit.ref(go));
      while (parent != null) {
        depth++;
        parent = edit.native.parent(parent);
      }
      return depth;
    });
  }

  void _copyScratch() {
    if (_scratch.length < _all.length) _scratch = List.filled(_all.length * 2, null);
    _scratch.setRange(0, _all.length, _all);
  }

  /// One fixed tick's callbacks, right before `step()`.
  void fixedUpdate(int tick, List<EngineEvent> events) =>
      world._runPhase(GameplayPhase.fixedUpdate, tick, () => _fixedUpdate(tick, events));
  void _fixedUpdate(int tick, List<EngineEvent> events) {
    settle();
    if (world.isDisposed) return;
    final ctx = FixedUpdateContext(tick: tick, dt: world.fixedDt, events: events, context: world.context);
    _startPending(ctx.context);
    if (world.isDisposed) return;
    _run(ctx.context, 'fixedUpdate', (b) => b.fixedUpdate(ctx));
  }

  /// Right after `step()`: the behaviours of the objects the tick made are
  /// created, then the contact callbacks of the objects involved (in
  /// contact order, side `a` then `b`), then `afterPhysics` for the
  /// behaviours that opted in.
  void afterPhysics(
    int tick,
    List<Contact> contacts,
    List<ZoneEvent> zones,
    List<AnimationEnded> animations,
    List<AnimationMarker> markers,
    List<PlaybackEnded> playback, [
    List<AnimatorStateEvent> states = const [],
  ]) {
    world._runPhase(GameplayPhase.afterPhysics, tick, () {
      // Objects the tick made are created (onCreate/onEnable) before its
      // events reach anyone, so a pair made overlapping gets its enter on
      // both sides, as Unity's Awake runs before the next physics step
      // (foundation 2.13).
      settle();
      if (world.isDisposed) return;
      if (contacts.isNotEmpty) _dispatchContacts(contacts);
      if (zones.isNotEmpty) _dispatchZones(zones);
      if (animations.isNotEmpty) _dispatchAnimations(animations);
      if (states.isNotEmpty) _dispatchStates(states);
      if (markers.isNotEmpty) _dispatchMarkers(markers);
      if (playback.isNotEmpty) _dispatchPlayback(playback);
      final ctx = AfterPhysicsContext(tick: tick, contacts: contacts, context: world.context);
      _run(ctx.context, 'afterPhysics', (b) {
        if (b.wantsAfterPhysics) b.afterPhysics(ctx);
      });
      if (_routines.isNotEmpty) ctx.context.run(() => _resume(fixed: true));
    });
  }

  void _dispatchContacts(List<Contact> contacts) {
    if (_all.isEmpty) return;
    world.context.run(() {
      for (final c in contacts) {
        // Each collider's object, then its body's when that is another
        // object (a child collider joined its parent's Rigidbody,
        // ADR-0034) — once each, as Unity calls both.
        _contactTo(world._byEntity[c.a], c);
        // A controller hit is the character's alone, as Unity's.
        if (c.kind == ContactKind.controllerHit) continue;
        final bodyA = c.bodyA;
        if (bodyA != null && bodyA != c.a) _contactTo(world._byEntity[bodyA], c);
        if (c.b != c.a) _contactTo(world._byEntity[c.b], c);
        final bodyB = c.bodyB;
        if (bodyB != null && bodyB != c.b && bodyB != bodyA) _contactTo(world._byEntity[bodyB], c);
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
            world._callbackGameplay(() => b.onAnimationEnded(e));
          } catch (err, st) {
            _threw(b, 'onAnimationEnded', err, st);
          }
        }
      }
    });
  }

  void _dispatchStates(List<AnimatorStateEvent> states) {
    if (_all.isEmpty) return;
    world.context.run(() {
      for (final s in states) {
        final go = world._byEntity[s.entity];
        if (go == null || go._behaviours.isEmpty) continue;
        for (final b in go._behaviours.toList(growable: false)) {
          if (!b._active || b._destroyRequested) continue;
          final entered = s is AnimatorStateEntered;
          try {
            world._callbackGameplay(() {
              switch (s) {
                case AnimatorStateEntered():
                  b.onAnimatorStateEntered(s);
                case AnimatorStateExited():
                  b.onAnimatorStateExited(s);
              }
            });
          } catch (err, st) {
            _threw(b, entered ? 'onAnimatorStateEntered' : 'onAnimatorStateExited', err, st);
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
            world._callbackGameplay(() => b.onAnimationMarker(m));
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
            world._callbackGameplay(() => b.onPlaybackEnded(e));
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
        world._callbackGameplay(() {
          switch (z) {
            case ZoneEntered():
              b.onZoneEnter(z);
            case ZoneExited():
              b.onZoneExit(z);
          }
        });
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
        world._callbackGameplay(() {
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
            case ContactKind.triggerStay:
              b.onTriggerStay(c);
            case ContactKind.controllerHit:
              b.onControllerColliderHit(c);
          }
        });
      } catch (e, st) {
        _threw(b, 'on${c.kind.name[0].toUpperCase()}${c.kind.name.substring(1)}', e, st);
      }
    }
  }

  /// Once per host frame: every `update`, then every `lateUpdate`.
  void update(double dt) {
    final ctx = UpdateContext(dt: dt, alpha: world.alpha, tick: world.tick, context: world.context);
    world._runPhase(GameplayPhase.update, ctx.tick, () {
      settle();
      if (world.isDisposed) return;
      _startPending(ctx.context);
      if (world.isDisposed) return;
      _run(ctx.context, 'update', (b) => b.update(ctx));
      if (_routines.isNotEmpty) ctx.context.run(() => _resume(fixed: false));
    });
    if (!world.isDisposed) {
      world._runPhase(
        GameplayPhase.lateUpdate,
        ctx.tick,
        () => _run(ctx.context, 'lateUpdate', (b) => b.lateUpdate(ctx)),
      );
    }
  }

  /// Resumes the coroutines whose wait is over: every kind but
  /// [WaitForFixedUpdate] after the frame's `update`s, those after a
  /// tick's `afterPhysics`. Ones started during the pass wait for the next.
  void _resume({required bool fixed}) {
    if (world.isDisposed) return;
    final n = _routines.length;
    for (var i = 0; i < n && i < _routines.length; i++) {
      if (world.isDisposed) return;
      final c = _routines[i];
      if (c._target case final t? when !t.isAlive) c._done = true;
      if (c._done || _fixedWait(c._wait) != fixed) continue;
      try {
        if (_ready(c)) _advance(c);
      } catch (e, st) {
        _routineThrew(c, e, st);
      }
    }
    _routines.removeWhere((c) {
      if (!c._done) return false;
      c._owner?._routines.remove(c);
      c._finish();
      return true;
    });
  }

  bool _fixedWait(Object? wait) =>
      wait is WaitForFixedUpdate || (wait is WaitForSeconds && wait.clock == TimeClock.simulation);

  bool _ready(Coroutine c) {
    final w = c._wait;
    if (w is WaitForFixedUpdate) return world.tick > c._tick;
    if (w is WaitForSeconds && w.clock == TimeClock.simulation) {
      return world.tick > c._tick && _seconds(w.clock) + 1e-6 >= c._until;
    }
    if (world._frameCount <= c._frame) return false;
    return switch (w) {
      // A microsecond's slack: frame times summed in floating point.
      WaitForSeconds(:final clock) => _seconds(clock) + 1e-6 >= c._until,
      WaitUntil(:final condition) => world._callbackGameplay(condition),
      WaitWhile(:final condition) => !world._callbackGameplay(condition),
      Coroutine() => !w.isRunning,
      _ => true,
    };
  }

  /// Starts a coroutine no behaviour owns (a tween on an object or on the
  /// world): at the next scheduler boundary, as [Behaviour.startCoroutine].
  Coroutine _start(Coroutine c) {
    return world._withGameplay(() => world._gameplay!.schedule(c));
  }

  void _disposeRoutines() {
    for (final c in _routines) {
      c._owner?._routines.remove(c);
      c._finish();
    }
    _routines.clear();
  }

  // Engine-only bookkeeping: safe while staging, with no iterator or callback.
  void _arm(Coroutine c, Object? wait) {
    c._wait = wait;
    c._frame = world._frameCount;
    c._tick = world.tick;
    if (wait is WaitForSeconds) {
      _checkSeconds(wait.seconds);
      c._until = _seconds(wait.clock) + wait.seconds;
    }
  }

  /// Runs [c] to its next yield (nested generators in place).
  void _advance(Coroutine c) {
    if (!c.isRunning) return;
    try {
      world._callbackGameplay(() {
        if (c._routine case final routine?) {
          c._routine = null;
          final iterator = routine.iterator;
          if (!c.isRunning) return;
          c._stack.add(iterator);
        }
        while (c.isRunning) {
          final top = c._stack.last;
          final more = top.moveNext();
          if (!c.isRunning) return;
          if (!more) {
            c._stack.removeLast();
            if (c._stack.isEmpty) c._done = true;
            continue;
          }
          final v = top.current;
          if (!c.isRunning) return;
          if (v is Iterable<Object?>) {
            final iterator = v.iterator;
            if (!c.isRunning) return;
            c._stack.add(iterator);
            continue;
          }
          _arm(c, v);
          return;
        }
      });
    } catch (e, st) {
      _routineThrew(c, e, st);
    }
  }

  void _routineThrew(Coroutine c, Object e, StackTrace st) {
    if (e is _GameplayCallbackEntrySkipped) return;
    c._done = true;
    final callback = c._invoke ? 'invoke' : (c._owner == null ? 'tween' : 'coroutine');
    world.onBehaviourError(BehaviourError(c._owner, callback, e, st, tick: world.tick, gameObject: c._target));
  }

  /// Runs `start` the first time; false when it threw (reported, and the
  /// callback after it skipped this once — Unity's order). After
  /// [_startPending] this only reports that throw.
  bool _startIfNeeded(Behaviour b) {
    if (b._started) {
      if (!b._startThrew) return true;
      b._startThrew = false;
      return false;
    }
    b._started = true;
    return _call(b, 'start', b.start);
  }

  /// Every active behaviour's pending `start`, in callback order, before
  /// the pass's first `fixedUpdate` or `update` (docs/foundation.md 1.8):
  /// as in Unity, a script early in the order sees the `start` of one after
  /// it. Nothing to do (and no allocation) when all have started.
  void _startPending(GameContext context) {
    if (!_entryReady()) return;
    var pending = false;
    for (final b in _all) {
      if (!b._started && b._active && !b._destroyRequested) {
        pending = true;
        break;
      }
    }
    if (!pending) return;
    _run(context, 'start', (b) {
      if (b._started || !_entryReady()) return;
      b._started = true;
      b._startThrew = !_call(b, 'start', b.start);
    }, transactional: false);
  }

  /// Calls [f], one of [b]'s callbacks; a throw goes to
  /// [World.onBehaviourError] and the caller carries on. False when it threw.
  bool _call(Behaviour b, String callback, void Function() f) {
    world._lifecycleDepth++;
    try {
      world._callbackGameplay(f);
      return true;
    } catch (e, st) {
      _threw(b, callback, e, st);
      return false;
    } finally {
      world._lifecycleDepth--;
    }
  }

  void _threw(Behaviour b, String callback, Object e, StackTrace st) {
    if (e is _GameplayCallbackEntrySkipped) return;
    world.onBehaviourError(BehaviourError(b, callback, e, st, tick: world.tick));
  }

  /// Runs [f] over the active behaviours in order, with the world's context
  /// current. Behaviours attached, disabled or destroyed during the pass are
  /// skipped for the rest of it (their transition applies at the next
  /// boundary).
  void _run(GameContext context, String callback, void Function(Behaviour) f, {bool transactional = true}) {
    final n = _all.length;
    if (n == 0 || world._callbackEntryBlocked || world.isDisposed) return;
    _copyScratch();
    context.run(() {
      for (var i = 0; i < n; i++) {
        if (world.isDisposed || world._callbackEntryBlocked) break;
        final b = _scratch[i]!;
        if (!b._active || b._destroyRequested || !b._gameObject!._alive) continue;
        final participation =
            (b._type == null ? world._registeredTypeOf(b) : world.registry.lookup(b._type!.id) ?? b._type)?.callbacks;
        // Generated participation describes periodic callbacks, not lifecycle.
        if (callback != 'start' && participation != null && !participation.contains(callback)) continue;
        if (callback == 'afterPhysics' && !b.wantsAfterPhysics) continue;
        // A start failure must abort its own scope before it is reported.
        if ((callback == 'fixedUpdate' || callback == 'update') && !_startIfNeeded(b)) continue;
        try {
          if (transactional) {
            world._callbackGameplay(() => f(b));
          } else {
            f(b);
          }
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
