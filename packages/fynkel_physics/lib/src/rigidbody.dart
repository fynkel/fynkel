part of 'physics.dart';

// Rigidbodies (docs/stage-5.md §3.3, step 4), as Unity's: a behaviour that
// lets physics move its object, with its colliders and those below it (down
// to the next rigidbody) as its shape, a mass they share by volume, forces
// in Unity's four modes, and the object's transform put where the step
// left the body.

/// How a push changes a body's motion (Unity's `ForceMode`).
enum ForceMode {
  /// Newtons (newton metres for a torque) over the next step: mass
  /// matters.
  force,

  /// Metres (radians) a second squared over the next step: mass doesn't.
  acceleration,

  /// Newton seconds, at once: mass matters.
  impulse,

  /// Metres (radians) a second, at once: mass doesn't.
  velocityChange,
}

/// How a rigidbody's object is drawn between steps (Unity's
/// `RigidbodyInterpolation`).
enum RigidbodyInterpolation {
  /// Where the last step left it: it moves in steps when frames come
  /// faster than steps.
  none,

  /// Between where the last two steps left it: smooth, a step behind.
  interpolate,

  /// Ahead of where the last step left it, by its velocity: smooth, and
  /// not behind, but it can go into what it's about to hit.
  extrapolate,
}

/// How a rigidbody finds what it hits (Unity's `CollisionDetectionMode`).
/// Every mode but [discrete] stops a fast body at thin things it would
/// otherwise pass between two steps. The three continuous modes do the
/// same here, Rapier's continuous detection against everything; they're
/// kept so code from Unity reads the same.
enum CollisionDetectionMode {
  /// Checked once a step: a fast, small body can pass through thin things.
  discrete,

  /// Swept between steps, so it stops at what it would pass through.
  continuous,

  /// As [continuous].
  continuousDynamic,

  /// As [continuous].
  continuousSpeculative,
}

/// The axes physics doesn't move or turn a rigidbody on, in the world's
/// axes (Unity's `RigidbodyConstraints`, with its values). Join them with
/// `|`: `RigidbodyConstraints.freezePositionY | RigidbodyConstraints.freezeRotation`.
extension type const RigidbodyConstraints(int value) {
  /// Nothing frozen.
  static const none = RigidbodyConstraints(0);

  /// It doesn't move along the world's X axis.
  static const freezePositionX = RigidbodyConstraints(2);

  /// It doesn't move along the world's Y axis.
  static const freezePositionY = RigidbodyConstraints(4);

  /// It doesn't move along the world's Z axis.
  static const freezePositionZ = RigidbodyConstraints(8);

  /// It doesn't turn about the world's X axis.
  static const freezeRotationX = RigidbodyConstraints(16);

  /// It doesn't turn about the world's Y axis.
  static const freezeRotationY = RigidbodyConstraints(32);

  /// It doesn't turn about the world's Z axis.
  static const freezeRotationZ = RigidbodyConstraints(64);

  /// It doesn't move at all, but it turns.
  static const freezePosition = RigidbodyConstraints(14);

  /// It doesn't turn at all, but it moves.
  static const freezeRotation = RigidbodyConstraints(112);

  /// It neither moves nor turns.
  static const freezeAll = RigidbodyConstraints(126);

  /// Both: every axis frozen in this or in [other].
  RigidbodyConstraints operator |(RigidbodyConstraints other) => RigidbodyConstraints(value | other.value);

  /// Whether every axis [other] freezes is frozen here.
  bool has(RigidbodyConstraints other) => value & other.value == other.value;
}

/// Lets physics move its object (Unity's `Rigidbody`): gravity pulls it,
/// forces push it, and it collides through its colliders, those on its
/// object and below it down to the next rigidbody. Each step puts the
/// object where the body is; set the object's transform (or [position])
/// and the body is put there at once.
///
/// It's in the world while it's enabled and its object is shown.
final class Rigidbody extends Behaviour {
  /// A rigidbody with Unity's defaults: 1 kg, pulled by gravity, dynamic,
  /// no linear and 0.05 angular damping. A bad value throws an
  /// [ArgumentError].
  Rigidbody({
    this._mass = 1,
    this._useGravity = true,
    this._isKinematic = false,
    this._linearDamping = 0,
    this._angularDamping = 0.05,
    this._maxAngularVelocity = 50,
    this._centerOfMass,
    this._constraints = RigidbodyConstraints.none,
    this._interpolation = RigidbodyInterpolation.none,
    this._collisionDetectionMode = CollisionDetectionMode.discrete,
  }) {
    _check();
  }

  /// In kilograms, shared out over its colliders by their volumes: 1
  /// unless set; finite and more than 0, or it throws an [ArgumentError].
  double get mass => _mass;
  set mass(double value) => _setOrUndo(_mass, value, (v) => _mass = v, _apply);
  double _mass;

  /// Whether gravity ([Physics.gravity]) pulls it.
  bool get useGravity => _useGravity;
  set useGravity(bool value) => _setOrUndo(_useGravity, value, (v) => _useGravity = v, _apply);
  bool _useGravity;

  /// Whether the game moves it ([movePosition], [moveRotation], or its
  /// object's transform), not forces and contacts. It still pushes the
  /// bodies it moves into.
  bool get isKinematic => _isKinematic;
  set isKinematic(bool value) => _setOrUndo(_isKinematic, value, (v) => _isKinematic = v, _apply);
  bool _isKinematic;

  /// How fast it slows down as it moves, per second: 0 (unless set) for
  /// not at all. Finite and 0 or more, or it throws an [ArgumentError].
  double get linearDamping => _linearDamping;
  set linearDamping(double value) => _setOrUndo(_linearDamping, value, (v) => _linearDamping = v, _apply);
  double _linearDamping;

  /// How fast it slows down as it turns, per second: 0.05 unless set.
  /// Finite and 0 or more, or it throws an [ArgumentError].
  double get angularDamping => _angularDamping;
  set angularDamping(double value) => _setOrUndo(_angularDamping, value, (v) => _angularDamping = v, _apply);
  double _angularDamping;

  /// The fastest it turns, in radians a second. Rapier also turns a body
  /// no more than a quarter turn a step, 39 radians a second at 50 steps a
  /// second, where Unity doesn't: a small ball rolling fast (a bowling ball
  /// past 4 m/s) skids and slows. Take more steps ([Time.fixedDelta]) for
  /// such a game. 50 unless set; finite and 0 or more, or it throws an
  /// [ArgumentError].
  double get maxAngularVelocity => _maxAngularVelocity;
  set maxAngularVelocity(double value) =>
      _setOrUndo(_maxAngularVelocity, value, (v) => _maxAngularVelocity = v, _apply);
  double _maxAngularVelocity;

  /// The axes physics doesn't move or turn it on.
  RigidbodyConstraints get constraints => _constraints;
  set constraints(RigidbodyConstraints value) => _setOrUndo(_constraints, value, (v) => _constraints = v, _apply);
  RigidbodyConstraints _constraints;

  /// How its object is drawn between steps.
  RigidbodyInterpolation get interpolation => _interpolation;
  set interpolation(RigidbodyInterpolation value) =>
      _setOrUndo(_interpolation, value, (v) => _interpolation = v, _apply);
  RigidbodyInterpolation _interpolation;

  /// How it finds what it hits: anything but discrete for a fast, small
  /// body that mustn't pass through thin things.
  CollisionDetectionMode get collisionDetectionMode => _collisionDetectionMode;
  set collisionDetectionMode(CollisionDetectionMode value) =>
      _setOrUndo(_collisionDetectionMode, value, (v) => _collisionDetectionMode = v, _apply);
  CollisionDetectionMode _collisionDetectionMode;

  /// Its center of mass in its object's space: where it was set, or where
  /// its colliders put it. Setting it keeps it there until
  /// [resetCenterOfMass]. Reading the one its colliders put throws a
  /// [StateError] while it isn't in the world.
  Vec3 get centerOfMass {
    if (_centerOfMass case final set?) return set;
    final s = _readState('centerOfMass');
    final p = s.pose.position.get();
    return s.pose.rotation.get().inverse.rotate(s.center_of_mass.get() - p);
  }

  set centerOfMass(Vec3 value) => _setOrUndo(_centerOfMass, value, (v) => _centerOfMass = v, _apply);
  Vec3? _centerOfMass;

  /// Lets its colliders put its center of mass again.
  void resetCenterOfMass() => _setOrUndo(_centerOfMass, null, (v) => _centerOfMass = v, _apply);

  /// Its center of mass, in the world. Throws a [StateError] while it isn't
  /// in the world.
  Vec3 get worldCenterOfMass => _readState('worldCenterOfMass').center_of_mass.get();

  /// How fast it moves, in metres a second, in the world. Set before it's
  /// in the world, it starts at it.
  Vec3 get linearVelocity => _id == 0 ? _startLinear : _readState('linearVelocity').linear_velocity.get();
  set linearVelocity(Vec3 value) => _setVelocities(value, null);

  /// How fast it turns, in radians a second, about the world's axes. Set
  /// before it's in the world, it starts at it.
  Vec3 get angularVelocity => _id == 0 ? _startAngular : _readState('angularVelocity').angular_velocity.get();
  set angularVelocity(Vec3 value) => _setVelocities(null, value);

  // The velocities it starts at, set before it was in the world.
  var _startLinear = Vec3.zero;
  var _startAngular = Vec3.zero;

  void _setVelocities(Vec3? linear, Vec3? angular) {
    if (_id == 0) {
      _startLinear = linear ?? _startLinear;
      _startAngular = angular ?? _startAngular;
      return;
    }
    final state = _readState('velocities');
    final physics = _physics!;
    physics._vec.ref.set(linear ?? state.linear_velocity.get());
    physics._vec2.ref.set(angular ?? state.angular_velocity.get());
    checkPhysics(fp_rigidbody_velocities_set(physics._world, _id, physics._vec, physics._vec2));
  }

  /// Where the body is, in the world (Unity's `Rigidbody.position`): where
  /// the last step left it. Its object is there too, unless the body is
  /// interpolated or extrapolated: then the object's position (as
  /// `gameObject.position`) is drawn between steps. Setting it puts the
  /// body and its object there at once. While it isn't in the world
  /// (before its object is added, or disabled), it's its object's
  /// position, as a [Behaviour]'s is, and the body starts there.
  @override
  Vec3 get position => _id == 0 ? super.position : _readState('position').pose.position.get();
  @override
  set position(Vec3 value) {
    if (_id == 0) {
      super.position = value;
    } else {
      _teleport(value, _moved ? gameObject.worldTransform.rotation : rotation, 'position');
    }
  }

  /// How the body is turned, in the world: where the last step left it, as
  /// [position]. Setting it turns the body and its object at once. While
  /// it isn't in the world, it's its object's rotation.
  @override
  Quat get rotation => _id == 0 ? super.rotation : _readState('rotation').pose.rotation.get();
  @override
  set rotation(Quat value) {
    if (_id == 0) {
      super.rotation = value;
    } else {
      _teleport(_moved ? gameObject.worldTransform.position : position, value, 'rotation');
    }
  }

  /// Whether the game moved its object since physics last looked: then the
  /// object, not the body, says where the rest of it is.
  bool get _moved => _id != 0 && _place.chain() != _place.stamp;

  /// Whether it's asleep: still for a while, so the step skips it until
  /// something touches it or the game pushes it. False while it isn't in
  /// the world.
  bool get isSleeping => _physics != null && _id != 0 && _readState('isSleeping').sleeping == 1;

  /// Puts it to sleep. Does nothing while it isn't in the world, as
  /// Unity's.
  void sleep() => _setSleeping(1);

  /// Wakes it. Does nothing while it isn't in the world, as Unity's.
  void wakeUp() => _setSleeping(0);

  void _setSleeping(int asleep) {
    final physics = _physics;
    if (physics == null || _id == 0) return;
    checkPhysics(fp_rigidbody_sleep_set(physics._world, _id, asleep));
  }

  /// Pushes it with [force] through its center of mass, in the world.
  ///
  /// A [ForceMode.force] or [ForceMode.acceleration] push lasts one step,
  /// so call it in `onFixedUpdate`, once a step: from `onUpdate` it acts
  /// on some steps twice and on others not at all. An impulse or velocity
  /// change acts once, so call it anywhere. Throws a [StateError] while it
  /// isn't in the world, as the other pushes do.
  void addForce(Vec3 force, {ForceMode mode = ForceMode.force}) =>
      _pushWith('addForce', FpPushKind.FP_PUSH_KIND_FORCE, force, Vec3.zero, mode);

  /// Pushes it with [force] in its own space.
  void addRelativeForce(Vec3 force, {ForceMode mode = ForceMode.force}) {
    _inWorld('addRelativeForce');
    addForce(rotation.rotate(force), mode: mode);
  }

  /// Turns it with [torque] about the world's axes.
  void addTorque(Vec3 torque, {ForceMode mode = ForceMode.force}) =>
      _pushWith('addTorque', FpPushKind.FP_PUSH_KIND_TORQUE, torque, Vec3.zero, mode);

  /// Turns it with [torque] about its own axes.
  void addRelativeTorque(Vec3 torque, {ForceMode mode = ForceMode.force}) {
    _inWorld('addRelativeTorque');
    addTorque(rotation.rotate(torque), mode: mode);
  }

  /// Pushes it with [force] at [position], in the world: off its center of
  /// mass, it turns it too.
  void addForceAtPosition(Vec3 force, Vec3 position, {ForceMode mode = ForceMode.force}) =>
      _pushWith('addForceAtPosition', FpPushKind.FP_PUSH_KIND_AT_POSITION, force, position, mode);

  /// Pushes it away from an explosion at [explosionPosition]: [explosionForce]
  /// strong there, less further out, and nothing past [explosionRadius]
  /// (everywhere the same with 0). [upwardsModifier] puts the explosion
  /// that far below its center, so it lifts too.
  void addExplosionForce(
    double explosionForce,
    Vec3 explosionPosition,
    double explosionRadius, {
    double upwardsModifier = 0,
    ForceMode mode = ForceMode.force,
  }) {
    final physics = _inWorld('addExplosionForce');
    physics._push.ref
      ..force = explosionForce
      ..radius = explosionRadius
      ..upwards = upwardsModifier;
    _pushWith('addExplosionForce', FpPushKind.FP_PUSH_KIND_EXPLOSION, Vec3.zero, explosionPosition, mode);
  }

  /// Moves a kinematic body to [position] through the next step, pushing
  /// what it meets on the way; a dynamic one goes there at once, as if set
  /// there: it doesn't push what's on the way. Throws a [StateError] while
  /// it isn't in the world.
  void movePosition(Vec3 position) {
    final physics = _inWorld('movePosition');
    final rotation = _moveAskedAt == physics._steps ? _moveRotation : this.rotation;
    _move(physics, position, rotation);
  }

  /// Turns a kinematic body to [rotation] through the next step, as
  /// [movePosition] moves it.
  void moveRotation(Quat rotation) {
    final physics = _inWorld('moveRotation');
    final position = _moveAskedAt == physics._steps ? _movePosition : this.position;
    _move(physics, position, rotation);
  }

  // Its world's physics while it's enabled, its id and key there, and where
  // it is in the physics' list; the colliders on it; where its object is.
  Physics? _physics;
  int _id = 0;
  int _key = 0;
  int _index = 0;
  // Where it is in the physics' interpolated list, or -1.
  int _interpolatedAt = -1;
  final _colliders = <Collider>[];
  final _place = _Place();

  // Where the last two steps left it, its velocities after the last, the
  // step that last moved it, and whether its object shows where it rests.
  var _p0 = Vec3.zero;
  var _p1 = Vec3.zero;
  var _r0 = Quat.identity;
  var _r1 = Quat.identity;
  var _v1 = Vec3.zero;
  var _w1 = Vec3.zero;
  var _movedAt = -1;
  var _resting = true;

  // Where [_write] puts its object, and whether the game left the object
  // alone since physics last looked.
  var _toPosition = Vec3.zero;
  var _toRotation = Quat.identity;
  var _clean = false;

  // Where movePosition and moveRotation send it, and the step count then:
  // what the other keeps, if both are asked before the next step.
  var _movePosition = Vec3.zero;
  var _moveRotation = Quat.identity;
  var _moveAskedAt = -1;

  @override
  void onEnable() {
    super.onEnable();
    final physics = gameObject.physics;
    _physics = physics;
    physics._addBody(this);
  }

  @override
  void onDisable() {
    super.onDisable();
    _physics?._removeBody(this);
    _physics = null;
  }

  void _apply() {
    _check();
    _physics?._respecBody(this);
  }

  void _check() {
    if (!(_mass > 0) || !_mass.isFinite) throw ArgumentError.value(_mass, 'mass', 'must be finite and more than 0');
    for (final (name, value) in [
      ('linearDamping', _linearDamping),
      ('angularDamping', _angularDamping),
      ('maxAngularVelocity', _maxAngularVelocity),
    ]) {
      if (!(value >= 0) || !value.isFinite) throw ArgumentError.value(value, name, 'must be finite and 0 or more');
    }
    if (_constraints.value & ~RigidbodyConstraints.freezeAll.value != 0) {
      throw ArgumentError.value(_constraints.value, 'constraints', 'has bits that are no constraint');
    }
  }

  void _writeSpec(FpBodySpec spec) {
    spec
      ..kinematic = _isKinematic ? 1 : 0
      ..mass = _mass
      ..use_gravity = _useGravity ? 1 : 0
      ..linear_damping = _linearDamping
      ..angular_damping = _angularDamping
      ..max_angular_velocity = _maxAngularVelocity
      ..has_center_of_mass = _centerOfMass == null ? 0 : 1
      ..constraints = _constraints.value
      ..continuous = _collisionDetectionMode == CollisionDetectionMode.discrete ? 0 : 1;
    spec.center_of_mass.set(_centerOfMass ?? Vec3.zero);
  }

  Physics _inWorld(String what) {
    if (_physics case final physics? when _id != 0) return physics;
    throw _notInWorld(this, what);
  }

  FpBodyState _readState(String what) {
    final physics = _inWorld(what);
    checkPhysics(fp_rigidbody_state_get(physics._world, _id, physics._state));
    return physics._state.ref;
  }

  void _pushWith(String what, int kind, Vec3 vector, Vec3 position, ForceMode mode) {
    final physics = _inWorld(what);
    final push = physics._push.ref
      ..kind = kind
      ..mode = mode.index;
    push.vector.set(vector);
    push.position.set(position);
    checkPhysics(fp_rigidbody_push(physics._world, _id, physics._push));
  }

  void _move(Physics physics, Vec3 position, Quat rotation) {
    physics._writePose(position, rotation);
    checkPhysics(fp_rigidbody_move(physics._world, _id, physics._pose));
    _movePosition = position;
    _moveRotation = rotation;
    _moveAskedAt = physics._steps;
  }

  void _teleport(Vec3 position, Quat rotation, String what) {
    final physics = _inWorld(what);
    physics._writePose(position, rotation);
    checkPhysics(fp_body_pose_set(physics._world, _id, physics._pose.ref));
    _reset(position, rotation);
    _place.stamp = _place.chain();
    _put(position, rotation);
  }

  /// [position] and [rotation] in the world, moved from where its object
  /// is drawn to where the last step left it, as the native world has it:
  /// the same against both. They differ only while it's interpolated.
  (Vec3, Quat) _againstStep(Vec3 position, Quat rotation) {
    final drawn = gameObject.worldTransform;
    final turn = _r1 * drawn.rotation.inverse;
    return (_p1 + turn.rotate(position - drawn.position), (turn * rotation).normalized);
  }

  /// [_againstStep] on the position and rotation at [at], as
  /// [World.readWorldTransforms] writes them.
  void _againstStepAt(Pointer<Float> at) {
    final (position, rotation) = _againstStep(Vec3(at[0], at[1], at[2]), Quat(at[3], at[4], at[5], at[6]));
    at[0] = position.x;
    at[1] = position.y;
    at[2] = position.z;
    at[3] = rotation.x;
    at[4] = rotation.y;
    at[5] = rotation.z;
    at[6] = rotation.w;
  }

  /// It's at [position] and [rotation] now, not moving between steps.
  void _reset(Vec3 position, Quat rotation) {
    _p0 = _p1 = position;
    _r0 = _r1 = rotation;
    _resting = false;
  }

  /// The step numbered [step] left it at [position] and [rotation], moving
  /// at [linear] and [angular].
  void _stepped(Vec3 position, Quat rotation, Vec3 linear, Vec3 angular, int step) {
    _p0 = _movedAt == step - 1 ? _p1 : position;
    _r0 = _movedAt == step - 1 ? _r1 : rotation;
    _p1 = position;
    _r1 = rotation;
    _v1 = linear;
    _w1 = angular;
    _movedAt = step;
    _resting = false;
  }

  /// Puts its object at [position] and [rotation] in the world, unless the
  /// game moved it since physics last looked: then the next step puts the
  /// body where the game did.
  void _put(Vec3 position, Quat rotation) {
    _prepare(position, rotation);
    _write();
  }

  /// Notes where [_write] puts its object, and whether the game moved it.
  /// For several bodies, [Physics._putAll] prepares them all before it
  /// writes any: a body above this one moving its object is not the game
  /// moving it.
  void _prepare(Vec3 position, Quat rotation) {
    _toPosition = position;
    _toRotation = rotation;
    final place = _place;
    _clean = place.chain() == place.stamp;
    if (!_clean) return;
    final colliders = _colliders;
    for (var i = 0; i < colliders.length; i++) {
      final c = colliders[i];
      c._clean = c._place.chain() == c._place.stamp;
    }
  }

  /// Puts its object where [_prepare] noted, against where its parent is
  /// now, if the game left it alone.
  void _write() {
    if (!_clean) return;
    _clean = false;
    final position = _toPosition;
    final rotation = _toRotation;
    final place = _place;
    final colliders = _colliders;
    final object = gameObject;
    final ancestors = place.ancestors;
    if (ancestors.isEmpty) {
      object.transform = object.transform.copyWith(position: position, rotation: rotation);
    } else {
      final parent = ancestors.first.worldTransform;
      final back = parent.rotation.inverse;
      final local = back.rotate(position - parent.position);
      final scale = parent.scale;
      object.transform = object.transform.copyWith(
        position: Vec3(local.x / scale.x, local.y / scale.y, local.z / scale.z),
        rotation: (back * rotation).normalized,
      );
    }
    place.seen();
    for (var i = 0; i < colliders.length; i++) {
      final c = colliders[i];
      if (c._clean) c._place.seen();
    }
  }
}

/// Where a collider's or rigidbody's object is in the hierarchy: the
/// objects above it, the nearest rigidbody, and when it last moved, as its
/// own and its ancestors' [GameObjectSystems.transformStamp]s say.
final class _Place {
  late GameObject _object;
  List<GameObject> ancestors = const [];
  Rigidbody? body;

  /// The highest stamp of the object and its ancestors when physics last
  /// looked, and the hierarchy then.
  int stamp = -1;
  int _walkedAt = -1;

  /// Finds the objects above [object], and the nearest rigidbody on it or
  /// above it but [without].
  void _walk(GameObject object, {Rigidbody? without}) {
    _object = object;
    _walkedAt = object.world.hierarchyVersion;
    final above = <GameObject>[];
    body = null;
    for (GameObject? o = object; o != null; o = o.parent) {
      if (!identical(o, object)) above.add(o);
      if (body == null) {
        final found = o.get<Rigidbody>();
        if (found != null && found._id != 0 && !identical(found, without)) body = found;
      }
    }
    ancestors = above;
  }

  /// Whether [object] is above this one, as the hierarchy was when it was
  /// last walked, or is now.
  bool isBelow(GameObject object) {
    if (_object.world.hierarchyVersion != _walkedAt) _walk(_object);
    final above = ancestors;
    for (var i = 0; i < above.length; i++) {
      if (identical(above[i], object)) return true;
    }
    return false;
  }

  /// The highest stamp of the object and its ancestors now.
  int chain() {
    var s = _object.transformStamp;
    final above = ancestors;
    for (var i = 0; i < above.length; i++) {
      final a = above[i].transformStamp;
      if (a > s) s = a;
    }
    return s;
  }

  /// Whether the object or one above it moved since [seen], or got a new
  /// parent: then the ancestors and the rigidbody are found again, if the
  /// hierarchy changed.
  bool moved(World world) {
    if (chain() == stamp) return false;
    if (world.hierarchyVersion != _walkedAt) _walk(_object);
    return true;
  }

  /// Physics has seen where the object is now.
  void seen() => stamp = chain();
}
