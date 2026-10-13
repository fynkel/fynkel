part of 'physics.dart';

// Joints (docs/stage-5.md §3.7), as Unity's: behaviours on a rigidbody's
// object that join it to another rigidbody, or to the world, as the two
// are when it's joined. A joint waits until both rigidbodies are in the
// world, and joins again when one comes back after leaving. It breaks
// when it holds harder than its break force or torque: it's taken off its
// object, which hears of it with [JointBreaks].

/// A hinge's limits, in radians from where it was joined, -pi to pi
/// (Unity's `JointLimits`): `JointLimits(min: -45.deg, max: 90.deg)`.
final class JointLimits {
  /// Limits from [min] to [max]; a hinge given a bad pair throws an
  /// [ArgumentError].
  const JointLimits({this.min = 0, this.max = 0});

  /// How far it turns back, in radians, -pi to [max]: 0 unless given.
  final double min;

  /// How far it turns on, in radians, [min] to pi: 0 unless given.
  final double max;
}

/// One of a [CharacterJoint]'s limits, in radians (Unity's
/// `SoftJointLimit`; its bounciness and contact distance aren't here).
final class SoftJointLimit {
  /// A limit at [limit]; a joint given a bad one throws an
  /// [ArgumentError].
  const SoftJointLimit({this.limit = 0});

  /// How far it turns, in radians: 0 unless given. Each of a
  /// [CharacterJoint]'s limits says its range.
  final double limit;
}

/// A hinge's motor (Unity's `JointMotor`): it turns the hinge at
/// [targetVelocity] radians a second, with no more than [force] newton
/// metres.
final class JointMotor {
  /// A motor turning at [targetVelocity] with up to [force]; a hinge given
  /// a bad one throws an [ArgumentError].
  const JointMotor({this.targetVelocity = 0, this.force = 0});

  /// How fast it turns the hinge, in radians a second: 0 unless given;
  /// finite.
  final double targetVelocity;

  /// The most it turns it with, in newton metres: 0 unless given; finite
  /// and 0 or more.
  final double force;
}

/// A hinge's spring (Unity's `JointSpring`): it turns the hinge to
/// [targetPosition] radians with [spring] newton metres a radian, slowed
/// by [damper] per radian a second.
final class JointSpring {
  /// A spring of [spring] and [damper] pulling to [targetPosition]; a hinge
  /// given a bad one throws an [ArgumentError].
  const JointSpring({this.spring = 0, this.damper = 0, this.targetPosition = 0});

  /// How strong it is, in newton metres a radian: 0 unless given; finite
  /// and 0 or more.
  final double spring;

  /// How much it slows the hinge, in newton metres per radian a second: 0
  /// unless given; finite and 0 or more.
  final double damper;

  /// The angle it pulls to, in radians from where the hinge was joined: 0
  /// unless given; finite.
  final double targetPosition;
}

/// What joins its object's [Rigidbody] to [connectedBody], or to the world
/// when that's null (Unity's `Joint`): [FixedJoint], [HingeJoint],
/// [SpringJoint] and [CharacterJoint]. Its object needs a rigidbody; the joint waits for it,
/// and for the connected one, to be in the world.
///
/// It holds the two as they are when it joins them: [anchor] on its own
/// body, and [connectedAnchor] where the anchor is then, unless it's set.
/// Changing the bodies, the anchors or a hinge's axis joins them again, as
/// they are then.
sealed class Joint extends Behaviour {
  Joint({
    this._connectedBody,
    this._anchor = Vec3.zero,
    this._connectedAnchor,
    this._enableCollision = false,
    this._breakForce = double.infinity,
    this._breakTorque = double.infinity,
  }) {
    _checkBreak();
  }

  /// The rigidbody it's joined to; null for the world.
  Rigidbody? get connectedBody => _connectedBody;
  set connectedBody(Rigidbody? value) => _rejoin(() => _connectedBody = value);
  Rigidbody? _connectedBody;

  /// Where it holds, in its object's space, scale included: (0.5, 0, 0) is
  /// the +X face of a unit cube however the object is scaled. The scale is
  /// the one its object has when it joins. (0, 0, 0) unless set.
  Vec3 get anchor => _anchor;
  set anchor(Vec3 value) => _rejoin(() => _anchor = value);
  Vec3 _anchor;

  /// Where it holds on the connected body, in that body's object's space,
  /// scale included as for [anchor] (in the world's with none): as set, or
  /// where [anchor] was when it joined, while
  /// [autoConfigureConnectedAnchor]. Before it joins, one not set reads
  /// (0, 0, 0).
  Vec3 get connectedAnchor {
    if (_connectedAnchor case final set?) return set;
    if (_id == 0) return Vec3.zero;
    final a = _readState().connected_anchor.get();
    final scale = _connectedBody?.gameObject.worldTransform.scale ?? Vec3.one;
    return Vec3(a.x / scale.x, a.y / scale.y, a.z / scale.z);
  }

  set connectedAnchor(Vec3 value) => _rejoin(() => _connectedAnchor = value);
  Vec3? _connectedAnchor;

  /// Whether [connectedAnchor] is where [anchor] is when it joins (true
  /// unless [connectedAnchor] is set). Setting it true forgets the set one.
  bool get autoConfigureConnectedAnchor => _connectedAnchor == null;
  set autoConfigureConnectedAnchor(bool value) {
    if (value == autoConfigureConnectedAnchor) return;
    _rejoin(() => _connectedAnchor = value ? null : connectedAnchor);
  }

  /// Whether its body and the connected one collide (false unless set).
  bool get enableCollision => _enableCollision;
  set enableCollision(bool value) => _respec(() => _enableCollision = value);
  bool _enableCollision;

  /// The force, in newtons, it breaks at: infinity for never.
  double get breakForce => _breakForce;
  set breakForce(double value) => _respec(() => _breakForce = value);
  double _breakForce;

  /// The torque, in newton metres, it breaks at: infinity for never.
  double get breakTorque => _breakTorque;
  set breakTorque(double value) => _respec(() => _breakTorque = value);
  double _breakTorque;

  /// The force it held its body with in the last step, in the world;
  /// zero while it isn't joined.
  Vec3 get currentForce => _id == 0 ? Vec3.zero : _readState().force.get();

  /// The torque it held its body with in the last step, about [anchor],
  /// in the world; zero while it isn't joined.
  Vec3 get currentTorque => _id == 0 ? Vec3.zero : _readState().torque.get();

  /// Whether it joins its bodies now: both are in the world, and it hasn't
  /// broken.
  bool get isJoined => _id != 0;

  // Its world's physics while it's enabled, its id there (0 while it
  // waits), its key, and the rigidbody it's on.
  Physics? _physics;
  int _id = 0;
  int _key = -1;
  Rigidbody? _body;

  int get _kind;

  @override
  void onEnable() {
    super.onEnable();
    final physics = gameObject.physics;
    _physics = physics;
    physics._addJoint(this);
  }

  @override
  void onDisable() {
    super.onDisable();
    _physics?._removeJoint(this);
    _physics = null;
  }

  void _checkBreak() {
    for (final (name, value) in [('breakForce', _breakForce), ('breakTorque', _breakTorque)]) {
      if (!(value > 0)) throw ArgumentError.value(value, name, 'must be more than 0 (infinity for never)');
    }
  }

  void _check() {}

  /// Changes what can change on the joint as it is.
  void _respec(void Function() change) {
    change();
    _checkBreak();
    _check();
    _physics?._respecJoint(this);
  }

  /// Changes what it joins, and joins them again as they are now.
  void _rejoin(void Function() change) {
    change();
    _physics?._rejoin(this);
  }

  /// Writes what it is, on [body], to [spec].
  void _writeSpec(FpJointSpec spec, Rigidbody body) {
    final scale = body.gameObject.worldTransform.scale;
    spec
      ..body = body._id
      ..connected = _connectedBody?._id ?? 0
      ..kind = _kind
      ..has_connected_anchor = _connectedAnchor == null ? 0 : 1
      ..enable_collision = _enableCollision ? 1 : 0
      ..break_force = _breakForce
      ..break_torque = _breakTorque
      ..use_limits = 0
      ..use_motor = 0
      ..use_spring = 0
      ..min_distance = 0
      ..max_distance = 0
      ..use_swing_limits = 0
      ..swing1 = 0
      ..swing2 = 0;
    spec.anchor.set(_scaled(_anchor, scale));
    final connectedScale = _connectedBody?.gameObject.worldTransform.scale ?? Vec3.one;
    spec.connected_anchor.set(_scaled(_connectedAnchor ?? Vec3.zero, connectedScale));
    spec.axis.set(const Vec3(1, 0, 0));
    spec.swing_axis.set(const Vec3(0, 1, 0));
  }

  static Vec3 _scaled(Vec3 v, Vec3 scale) => Vec3(v.x * scale.x, v.y * scale.y, v.z * scale.z);

  FpJointState _readState() {
    final physics = _physics!;
    checkPhysics(fp_joint_state_get(physics._world, _id, physics._jointState));
    return physics._jointState.ref;
  }
}

/// Holds its rigidbody to the connected one (or the world) as the two
/// were when it joined them, so they move as one (Unity's `FixedJoint`).
final class FixedJoint extends Joint {
  /// A fixed joint to [connectedBody], or to the world when null. A break
  /// force or torque of 0 or less throws an [ArgumentError].
  FixedJoint({
    super.connectedBody,
    super.anchor,
    super.connectedAnchor,
    super.enableCollision,
    super.breakForce,
    super.breakTorque,
  });

  @override
  int get _kind => FpJointKind.FP_JOINT_KIND_FIXED;
}

/// Lets its rigidbody turn only about [axis] through [anchor] (Unity's
/// `HingeJoint`): a door, a wheel, a chain's link. It can stop at
/// [limits], be turned by a [motor], or be pulled to an angle by a
/// [spring]; the motor turns it in place of the spring when both are on.
final class HingeJoint extends Joint {
  /// A hinge to [connectedBody], or to the world when null, about [axis]
  /// (X unless given). A bad value throws an [ArgumentError].
  HingeJoint({
    super.connectedBody,
    super.anchor,
    super.connectedAnchor,
    this._axis = const Vec3(1, 0, 0),
    this._useLimits = false,
    this._limits = const JointLimits(),
    this._useMotor = false,
    this._motor = const JointMotor(),
    this._useSpring = false,
    this._spring = const JointSpring(),
    super.enableCollision,
    super.breakForce,
    super.breakTorque,
  }) {
    _check();
  }

  /// The axis it turns about, in its object's space.
  Vec3 get axis => _axis;
  set axis(Vec3 value) {
    if (value.length < 1e-6) throw ArgumentError.value(value, 'axis', "can't be (0, 0, 0)");
    _rejoin(() => _axis = value);
  }

  Vec3 _axis;

  /// Whether it stops at [limits].
  bool get useLimits => _useLimits;
  set useLimits(bool value) => _respec(() => _useLimits = value);
  bool _useLimits;

  /// Where it stops, while [useLimits]: -pi to pi radians, the min no more
  /// than the max, or it throws an [ArgumentError]. 0 to 0 unless set.
  JointLimits get limits => _limits;
  set limits(JointLimits value) => _respec(() => _limits = value);
  JointLimits _limits;

  /// Whether [motor] turns it.
  bool get useMotor => _useMotor;
  set useMotor(bool value) => _respec(() => _useMotor = value);
  bool _useMotor;

  /// What turns it, while [useMotor]. A bad one throws an
  /// [ArgumentError]. Turns at 0 with no force unless set.
  JointMotor get motor => _motor;
  set motor(JointMotor value) => _respec(() => _motor = value);
  JointMotor _motor;

  /// Whether [spring] pulls it.
  bool get useSpring => _useSpring;
  set useSpring(bool value) => _respec(() => _useSpring = value);
  bool _useSpring;

  /// What pulls it to an angle, while [useSpring]. A bad one throws an
  /// [ArgumentError]. No strength unless set.
  JointSpring get spring => _spring;
  set spring(JointSpring value) => _respec(() => _spring = value);
  JointSpring _spring;

  /// How far it's turned from where it was joined, in radians, -pi to pi:
  /// its body about [axis], from the connected one. 0 while it isn't
  /// joined.
  double get angle => _id == 0 ? 0 : _readState().angle;

  /// How fast it turns, in radians a second; 0 while it isn't joined.
  double get velocity => _id == 0 ? 0 : _readState().velocity;

  @override
  int get _kind => FpJointKind.FP_JOINT_KIND_HINGE;

  @override
  void _check() {
    if (_axis.length < 1e-6) throw ArgumentError.value(_axis, 'axis', "can't be (0, 0, 0)");
    final JointLimits(:min, :max) = _limits;
    if (!(min >= -_halfTurn && max <= _halfTurn && min <= max)) {
      throw ArgumentError.value('$min to $max', 'limits', 'must be -pi to pi radians, the min no more than the max');
    }
    final JointMotor(:targetVelocity, :force) = _motor;
    if (!targetVelocity.isFinite || !(force >= 0) || !force.isFinite) {
      throw ArgumentError.value('$targetVelocity, $force', 'motor', 'needs a finite velocity and a force of 0 or more');
    }
    _checkSpring(_spring.spring, _spring.damper);
    if (!_spring.targetPosition.isFinite) {
      throw ArgumentError.value(_spring.targetPosition, 'spring.targetPosition', 'must be finite');
    }
  }

  @override
  void _writeSpec(FpJointSpec spec, Rigidbody body) {
    super._writeSpec(spec, body);
    final scale = body.gameObject.worldTransform.scale;
    spec.axis.set(Joint._scaled(_axis, scale).normalized);
    spec
      ..use_limits = _useLimits ? 1 : 0
      ..limit_min = _limits.min
      ..limit_max = _limits.max
      ..use_motor = _useMotor ? 1 : 0
      ..motor_velocity = _motor.targetVelocity
      ..motor_force = _motor.force
      ..use_spring = _useSpring ? 1 : 0
      ..spring = _spring.spring
      ..damper = _spring.damper
      ..target = _spring.targetPosition;
  }
}

/// Pulls its rigidbody's [anchor] towards the connected one's, as a
/// spring would, while the two are nearer than [minDistance] or further
/// apart than [maxDistance] (Unity's `SpringJoint`). With the connected
/// anchor where the anchor was (Unity's default), it keeps the two as far
/// apart as they were, springily.
final class SpringJoint extends Joint {
  /// A spring to [connectedBody], or to the world when null: Unity's 10
  /// newtons a metre and 0.2 damper unless given. A bad value throws an
  /// [ArgumentError].
  SpringJoint({
    super.connectedBody,
    super.anchor,
    super.connectedAnchor,
    this._spring = 10,
    this._damper = 0.2,
    this._minDistance = 0,
    this._maxDistance = 0,
    super.enableCollision,
    super.breakForce,
    super.breakTorque,
  }) {
    _check();
  }

  /// How strong it is, in newtons a metre: finite and 0 or more, or it
  /// throws an [ArgumentError].
  double get spring => _spring;
  set spring(double value) => _respec(() => _spring = value);
  double _spring;

  /// How much it slows the two, in newtons per metre a second: finite and
  /// 0 or more, or it throws an [ArgumentError].
  double get damper => _damper;
  set damper(double value) => _respec(() => _damper = value);
  double _damper;

  /// Nearer than this, in metres, it pushes the anchors apart: 0 or more,
  /// and no more than [maxDistance], or it throws an [ArgumentError]. 0
  /// unless set.
  double get minDistance => _minDistance;
  set minDistance(double value) => _respec(() => _minDistance = value);
  double _minDistance;

  /// Further apart than this, in metres, it pulls the anchors together:
  /// finite, and no less than [minDistance], or it throws an
  /// [ArgumentError]. 0 unless set.
  double get maxDistance => _maxDistance;
  set maxDistance(double value) => _respec(() => _maxDistance = value);
  double _maxDistance;

  @override
  int get _kind => FpJointKind.FP_JOINT_KIND_SPRING;

  @override
  void _check() {
    _checkSpring(_spring, _damper);
    if (!(_minDistance >= 0) || !(_maxDistance >= _minDistance) || !_maxDistance.isFinite) {
      throw ArgumentError.value(
        '$_minDistance to $_maxDistance',
        'minDistance, maxDistance',
        'must be finite and 0 or more, the min no more than the max',
      );
    }
  }

  @override
  void _writeSpec(FpJointSpec spec, Rigidbody body) {
    super._writeSpec(spec, body);
    spec
      ..use_spring = 1
      ..spring = _spring
      ..damper = _damper
      ..target = 0
      ..min_distance = _minDistance
      ..max_distance = _maxDistance;
  }
}

/// Lets its rigidbody turn every way about [anchor], as a ball in a socket
/// (Unity's `CharacterJoint`): a chain's link, a rope's, a ragdoll's limb.
/// It twists about [axis] between [lowTwistLimit] and [highTwistLimit],
/// and swings [swing1Limit] each way about [swingAxis] and [swing2Limit]
/// each way about the axis square to both, all in radians. A limit at pi
/// (-pi for the low twist) frees it that way: all four there make a free
/// ball joint. Unity's defaults: twist -20.deg to 70.deg, swing 40.deg.
final class CharacterJoint extends Joint {
  /// A ball joint to [connectedBody], or to the world when null, with
  /// Unity's limits unless given. A bad value throws an [ArgumentError].
  CharacterJoint({
    super.connectedBody,
    super.anchor,
    super.connectedAnchor,
    this._axis = const Vec3(1, 0, 0),
    this._swingAxis = const Vec3(0, 1, 0),
    this._lowTwistLimit = const SoftJointLimit(limit: -20 * _degree),
    this._highTwistLimit = const SoftJointLimit(limit: 70 * _degree),
    this._swing1Limit = const SoftJointLimit(limit: 40 * _degree),
    this._swing2Limit = const SoftJointLimit(limit: 40 * _degree),
    super.enableCollision,
    super.breakForce,
    super.breakTorque,
  }) {
    _check();
  }

  /// The axis it twists about, in its object's space.
  Vec3 get axis => _axis;
  set axis(Vec3 value) {
    _checkAxes(value, _swingAxis);
    _rejoin(() => _axis = value);
  }

  Vec3 _axis;

  /// The axis it swings about by [swing1Limit], in its object's space:
  /// made square to [axis].
  Vec3 get swingAxis => _swingAxis;
  set swingAxis(Vec3 value) {
    _checkAxes(_axis, value);
    _rejoin(() => _swingAxis = value);
  }

  Vec3 _swingAxis;

  /// How far it twists back about [axis], from where it was joined, in
  /// radians: -pi to [highTwistLimit] (-20.deg unless set).
  SoftJointLimit get lowTwistLimit => _lowTwistLimit;
  set lowTwistLimit(SoftJointLimit value) => _respec(() => _lowTwistLimit = value);
  SoftJointLimit _lowTwistLimit;

  /// How far it twists on about [axis], from where it was joined, in
  /// radians: [lowTwistLimit] to pi (70.deg unless set).
  SoftJointLimit get highTwistLimit => _highTwistLimit;
  set highTwistLimit(SoftJointLimit value) => _respec(() => _highTwistLimit = value);
  SoftJointLimit _highTwistLimit;

  /// How far it swings each way about [swingAxis], in radians: 0 to pi
  /// (40.deg unless set).
  SoftJointLimit get swing1Limit => _swing1Limit;
  set swing1Limit(SoftJointLimit value) => _respec(() => _swing1Limit = value);
  SoftJointLimit _swing1Limit;

  /// How far it swings each way about the axis square to [axis] and
  /// [swingAxis], in radians: 0 to pi (40.deg unless set).
  SoftJointLimit get swing2Limit => _swing2Limit;
  set swing2Limit(SoftJointLimit value) => _respec(() => _swing2Limit = value);
  SoftJointLimit _swing2Limit;

  @override
  int get _kind => FpJointKind.FP_JOINT_KIND_CHARACTER;

  static void _checkAxes(Vec3 axis, Vec3 swingAxis) {
    if (axis.length < 1e-6) throw ArgumentError.value(axis, 'axis', "can't be (0, 0, 0)");
    if (swingAxis.length < 1e-6 || axis.normalized.cross(swingAxis.normalized).length < 1e-3) {
      throw ArgumentError.value(swingAxis, 'swingAxis', 'must lie across the axis');
    }
  }

  @override
  void _check() {
    _checkAxes(_axis, _swingAxis);
    final (low, high) = (_lowTwistLimit.limit, _highTwistLimit.limit);
    if (!(low >= -_halfTurn && high <= _halfTurn && low <= high)) {
      throw ArgumentError.value(
        '$low to $high',
        'twist limits',
        'must be -pi to pi radians, the low no more than the high',
      );
    }
    for (final (name, swing) in [('swing1Limit', _swing1Limit.limit), ('swing2Limit', _swing2Limit.limit)]) {
      if (!(swing >= 0 && swing <= _halfTurn)) throw ArgumentError.value(swing, name, 'must be 0 to pi radians');
    }
  }

  @override
  void _writeSpec(FpJointSpec spec, Rigidbody body) {
    super._writeSpec(spec, body);
    final scale = body.gameObject.worldTransform.scale;
    spec.axis.set(Joint._scaled(_axis, scale).normalized);
    spec.swing_axis.set(Joint._scaled(_swingAxis, scale).normalized);
    spec
      ..use_limits = 1
      ..limit_min = _lowTwistLimit.limit
      ..limit_max = _highTwistLimit.limit
      ..use_swing_limits = 1
      ..swing1 = _swing1Limit.limit
      ..swing2 = _swing2Limit.limit;
  }
}

// Half a turn, with room for pi read back from 32-bit floats; a degree.
const _halfTurn = math.pi + 1e-6;
const _degree = math.pi / 180;

void _checkSpring(double spring, double damper) {
  if (!(spring >= 0) || !spring.isFinite) throw ArgumentError.value(spring, 'spring', 'must be finite and 0 or more');
  if (!(damper >= 0) || !damper.isFinite) throw ArgumentError.value(damper, 'damper', 'must be finite and 0 or more');
}

/// The callback for a joint breaking (Unity's `OnJointBreak`), for its
/// [GameObject] or a [Behaviour] on it: `class Lamp extends GameObject
/// with JointBreaks`. It comes once, at the next point contacts are told:
/// the joint is out of the physics then ([Joint.isJoined] is false) but
/// still on its object, and it's taken off right after, as Unity does.
/// What it throws goes to [World.errors].
mixin JointBreaks {
  /// [joint] broke, holding with [breakForce] newtons.
  void onJointBreak(Joint joint, double breakForce) {}
}
