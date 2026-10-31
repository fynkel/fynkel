part of 'physics.dart';

// The character controller (docs/stage-5.md §3.7), as Unity's: a capsule
// collider the game moves with [CharacterController.move], on Rapier's
// kinematic character controller. The native world works out how far it
// goes and pushes the bodies in its way; the controller then moves its
// object, and the capsule follows it as any collider does.

/// What a [CharacterController]'s move hit (Unity's `CollisionFlags`):
/// [sides], [above], [below], or a mix, `flags.has(CollisionFlags.below)`.
extension type const CollisionFlags._(int value) {
  /// Nothing.
  static const none = CollisionFlags._(0);

  /// Something on its sides: a wall.
  static const sides = CollisionFlags._(FP_COLLIDED_SIDES);

  /// Something above it: a ceiling.
  static const above = CollisionFlags._(FP_COLLIDED_ABOVE);

  /// Something below it: the ground.
  static const below = CollisionFlags._(FP_COLLIDED_BELOW);

  /// Whether every one of [flags] is in these.
  bool has(CollisionFlags flags) => value & flags.value == flags.value;

  /// Both: every flag in these or in [other].
  CollisionFlags operator |(CollisionFlags other) => CollisionFlags._(value | other.value);
}

/// A character's capsule that the game moves with [move], as Unity's
/// `CharacterController`: it slides along walls, walks up steps up to
/// [stepOffset] high and slopes up to [slopeLimit], stops at anything
/// steeper or higher, and pushes the rigidbodies it walks into. Gravity
/// doesn't move it: the game does, through [move], or [simpleMove] does it
/// for it.
///
/// It rides what it stands on ([ground]): when that moves or turns, by a
/// tween, a kinematic rigidbody or the game's code, the controller goes
/// with it at the end of the frame, after the tweens, so a moving platform
/// carries it without lag ([moveWithGround]).
///
/// It's a [Collider], a capsule [height] tall and [radius] round around
/// [center], standing up in its object's space; rigidbodies bump into it,
/// and it meets triggers as a kinematic rigidbody does. Mix
/// [ControllerHits] into its object or a behaviour on it to hear what each
/// move hits.
final class CharacterController extends Collider {
  /// A controller with Unity's defaults unless given: 2 m tall, 0.5 m
  /// round, slopes up to 45.deg, steps up to 0.3 m. A bad value throws an
  /// [ArgumentError].
  ///
  /// Its capsule turns with its object, all ways, where Unity's always
  /// stands up: keep its object upright, and turn a child for the model.
  CharacterController({
    this._height = 2,
    this._radius = 0.5,
    this._slopeLimit = 45 * _degree,
    this._stepOffset = 0.3,
    this._skinWidth = 0.08,
    this._mass = 70,
    super.center,
  }) {
    _checkShape();
    _checkMoving();
  }

  /// Its height, end to end, in metres, in its object's space: the
  /// object's scale multiplies it, as a [CapsuleCollider]'s. Unity's 2
  /// unless given; more than 0, or it throws an [ArgumentError].
  double get height => _height;
  set height(double value) => _setOrUndo(_height, value, (v) => _height = v, _changed);

  double _height;

  /// Its radius, in metres, in its object's space: the object's scale
  /// multiplies it, as a [CapsuleCollider]'s. 0.5 unless given; more than
  /// 0, or it throws an [ArgumentError].
  double get radius => _radius;
  set radius(double value) => _setOrUndo(_radius, value, (v) => _radius = v, _changed);

  double _radius;

  /// The steepest slope it walks up, in radians from flat (45.deg unless
  /// given): 0 to pi, or it throws an [ArgumentError], so `45` in degrees
  /// is refused; write `45.deg`.
  double get slopeLimit => _slopeLimit;
  set slopeLimit(double value) => _setOrUndo(_slopeLimit, value, (v) => _slopeLimit = v, _checkMoving);

  double _slopeLimit;

  /// The highest step it walks up, in metres (0.3 unless given). It also
  /// keeps to the ground going down one this high. Finite and 0 or more,
  /// or it throws an [ArgumentError].
  double get stepOffset => _stepOffset;
  set stepOffset(double value) => _setOrUndo(_stepOffset, value, (v) => _stepOffset = v, _checkMoving);

  double _stepOffset;

  /// The gap it keeps from what it meets, in metres (0.08 unless given):
  /// big enough not to get stuck, small enough not to be seen. Finite and
  /// more than 0, or it throws an [ArgumentError].
  double get skinWidth => _skinWidth;
  set skinWidth(double value) => _setOrUndo(_skinWidth, value, (v) => _skinWidth = v, _checkMoving);

  double _skinWidth;

  /// How heavy it is, in kilograms, when it pushes the rigidbodies it walks
  /// into (70 unless given): 0 pushes none, and it stops at them. Finite
  /// and 0 or more, or it throws an [ArgumentError].
  double get mass => _mass;
  set mass(double value) => _setOrUndo(_mass, value, (v) => _mass = v, _checkMoving);

  double _mass;

  /// What the last [move] hit.
  CollisionFlags get collisionFlags => _flags;
  var _flags = CollisionFlags.none;

  /// Whether it touched the ground in the last [move].
  bool get isGrounded => _flags.has(CollisionFlags.below);

  /// How fast the last [move] took it: how far it went, over the frame's
  /// time (the fixed step's, in a fixed update).
  Vec3 get velocity => _velocity;
  var _velocity = Vec3.zero;

  /// What it stood on after the last [move], or null when it wasn't
  /// grounded.
  Collider? get ground => _ground;
  Collider? _ground;

  /// Whether it goes with its [ground] when that moves or turns (true
  /// unless set): at the end of each frame, after the late updates'
  /// tweens, it moves by as much as the ground took the point it stands
  /// on, as far as what's in its way lets it.
  bool moveWithGround = true;

  /// How fast its [ground] carried it in the last frame.
  Vec3 get groundVelocity => _groundVelocity;
  var _groundVelocity = Vec3.zero;

  // Where the ground's object was when it was last looked at.
  var _groundAt = Vec3.zero;
  var _groundTurn = Quat.identity;

  // How fast [simpleMove] has it falling.
  var _fall = 0.0;

  /// Moves it by [motion], in the world, as far as what's in its way lets
  /// it: along walls, up steps and slopes it can climb, pushing rigidbodies.
  /// Its object moves with it. Says what it hit. Throws a [StateError]
  /// while it isn't in the world.
  CollisionFlags move(Vec3 motion) {
    final physics = _inWorld('move');
    physics._follow();
    final time = world.time;
    final seconds = time.delta > 0 ? time.delta : time.fixedDelta;
    final moved = _moveBy(physics, motion, seconds);
    final words = physics._characterWordsOut;
    _flags = CollisionFlags._(words[3]);
    _velocity = moved / seconds;
    final key = physics._characterMove.ref.ground_key;
    final ground = key != 0 && key < physics._byKey.length ? physics._byKey[key] : null;
    if (!identical(ground, _ground)) {
      _ground = ground;
      if (ground != null) {
        _groundAt = ground.gameObject.position;
        _groundTurn = ground.gameObject.rotation;
      }
    }
    final hits = words[4];
    final object = gameObject;
    if (hits > 0 && _listening(object)) physics._tellHits(this, hits, motion);
    return _flags;
  }

  /// Asks the native world how far [motion] goes, and moves the object so.
  Vec3 _moveBy(Physics physics, Vec3 motion, double seconds) {
    final floats = physics._characterFloats;
    floats[0] = math.min(_slopeLimit.inDegrees, 180);
    floats[1] = _stepOffset;
    floats[2] = _skinWidth;
    floats[3] = _mass;
    final vectors = physics._vectors;
    vectors[0] = motion.x;
    vectors[1] = motion.y;
    vectors[2] = motion.z;
    checkPhysics(
      fp_character_move(physics._world, _id, physics._character, physics._point, seconds, physics._characterMove),
    );
    final out = physics._characterFloatsOut;
    final moved = Vec3(out[0], out[1], out[2]);
    gameObject.position += moved;
    return moved;
  }

  /// Goes with the ground, after the frame's tweens and late updates.
  @override
  void onLateUpdate() {
    super.onLateUpdate();
    _groundVelocity = Vec3.zero;
    final ground = _ground;
    if (ground == null) return;
    if (ground._physics == null || ground._id == 0) {
      _ground = null;
      return;
    }
    final object = ground.gameObject;
    final (at, turn) = (object.position, object.rotation);
    if (at == _groundAt && turn == _groundTurn) return;
    final (wasAt, wasTurn) = (_groundAt, _groundTurn);
    _groundAt = at;
    _groundTurn = turn;
    if (!moveWithGround || _id == 0) return;
    final mine = gameObject.position;
    final carried = at + (turn * wasTurn.inverse).rotate(mine - wasAt) - mine;
    final physics = _physics!;
    physics._follow();
    final time = world.time;
    final seconds = time.delta > 0 ? time.delta : time.fixedDelta;
    _groundVelocity = _moveBy(physics, carried, seconds) / seconds;
  }

  /// Moves it at [speed] metres a second over the frame, along the ground
  /// (its Y is left out), and lets gravity pull it down while it's off the
  /// ground (Unity's `SimpleMove`). Says whether it's grounded.
  bool simpleMove(Vec3 speed) {
    final physics = _inWorld('simpleMove');
    final time = world.time;
    final seconds = time.delta > 0 ? time.delta : time.fixedDelta;
    // On the ground, a step's worth of gravity keeps it there.
    _fall = isGrounded ? physics.gravity.y * seconds : _fall + physics.gravity.y * seconds;
    move(Vec3(speed.x * seconds, _fall * seconds, speed.z * seconds));
    return isGrounded;
  }

  static bool _listening(GameObject object) {
    if (object is ControllerHits) return true;
    for (var i = 0; i < object.componentCount; i++) {
      if (object.componentAt(i) is ControllerHits) return true;
    }
    return false;
  }

  @override
  void _checkShape() {
    Collider._positive('radius', _radius);
    Collider._positive('height', _height);
  }

  void _checkMoving() {
    if (!(_slopeLimit >= 0 && _slopeLimit <= _halfTurn)) {
      throw ArgumentError.value(_slopeLimit, 'slopeLimit', 'must be 0 to pi radians');
    }
    if (!(_stepOffset >= 0) || !_stepOffset.isFinite) {
      throw ArgumentError.value(_stepOffset, 'stepOffset', 'must be finite and 0 or more');
    }
    Collider._positive('skinWidth', _skinWidth);
    if (!(_mass >= 0) || !_mass.isFinite) throw ArgumentError.value(_mass, 'mass', 'must be finite and 0 or more');
  }

  @override
  void _writeShape(FpColliderSpec spec) {
    spec
      ..kind = FpColliderKind.FP_COLLIDER_KIND_CAPSULE
      ..direction = CapsuleDirection.y.index;
    spec.size
      ..x = _radius
      ..y = _height
      ..z = 0;
  }
}

/// [CharacterController] callbacks (Unity's `OnControllerColliderHit`),
/// for its [GameObject] or a [Behaviour] on it: `class Player extends
/// GameObject with ControllerHits`. Each [CharacterController.move] calls
/// [onControllerColliderHit] once for each collider it hit, before it
/// returns. What one throws goes to [World.errors].
mixin ControllerHits {
  /// A move hit [hit]'s collider.
  void onControllerColliderHit(ControllerColliderHit hit) {}
}

/// What a [CharacterController]'s move hit (Unity's `ControllerColliderHit`,
/// but for [moveLength]: see there).
final class ControllerColliderHit {
  ControllerColliderHit._(this.controller, this.collider, this.point, this.normal, this.moveDirection, this.moveLength);

  /// The controller that moved.
  final CharacterController controller;

  /// The collider it hit.
  final Collider collider;

  /// Where it hit it, in the world.
  final Vec3 point;

  /// The hit surface's normal there, out of it.
  final Vec3 normal;

  /// Which way the move went, unit length, in the world: zero for no move.
  final Vec3 moveDirection;

  /// How far the move was asked to go, in metres: the length of the
  /// motion given to [CharacterController.move]. Unity's is how far the
  /// controller went before it hit the collider; physics doesn't say that
  /// here, so to tell a graze from a head-on stop, look at how [normal]
  /// faces [moveDirection].
  final double moveLength;

  /// The object hit.
  GameObject get gameObject => collider.gameObject;

  /// The rigidbody hit, if it's on one.
  Rigidbody? get rigidbody => collider.attachedRigidbody;
}

extension on Physics {
  /// Tells [controller]'s object and its behaviours of the [count]
  /// colliders the last move hit, asked to go [motion].
  void _tellHits(CharacterController controller, int count, Vec3 motion) {
    if (count > _characterHitCapacity) {
      if (_characterHits != nullptr) _memory.free(_characterHits);
      _characterHitCapacity = count * 2;
      _characterHits = _memory<FpCharacterHit>(_characterHitCapacity);
    }
    checkPhysics(fp_character_hits_read(_pointer, _characterHits, count));
    // An FpCharacterHit: two 64-bit ids, then floats 4–6 the point and 7–9
    // the normal, 10 floats in all.
    final ids = _characterHits.cast<Uint64>();
    final floats = _characterHits.cast<Float>();
    final length = motion.length;
    final direction = length > 0 ? motion / length : Vec3.zero;
    final hits = <ControllerColliderHit>[];
    for (var i = 0; i < count; i++) {
      final key = ids[i * 5 + 1];
      final collider = key < _byKey.length ? _byKey[key] : null;
      if (collider == null) continue;
      final f = i * 10;
      hits.add(
        ControllerColliderHit._(
          controller,
          collider,
          Vec3(floats[f + 4], floats[f + 5], floats[f + 6]),
          Vec3(floats[f + 7], floats[f + 8], floats[f + 9]),
          direction,
          length,
        ),
      );
    }
    final object = controller.gameObject;
    for (final hit in hits) {
      _hitTo(object, object, hit);
      for (var i = 0; i < object.componentCount; i++) {
        final c = object.componentAt(i);
        if (!c.isRemoved) _hitTo(object, c, hit);
      }
    }
  }

  void _hitTo(GameObject object, Object target, ControllerColliderHit hit) {
    if (target is! ControllerHits) return;
    try {
      target.onControllerColliderHit(hit);
    } catch (error, stack) {
      world.report(object, error, stack);
    }
  }
}
