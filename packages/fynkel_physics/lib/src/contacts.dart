part of 'physics.dart';

// Callbacks and the layer matrix (docs/stage-5.md §3.4, §3.5). After each
// step the native world lists the contacts that began, went on and ended
// between colliders someone listens on; the physics reads them into one
// buffer and calls each side's objects and behaviours: the collider's
// object and its rigidbody's. Between frames, that's at the start of the
// next frame, before the fixed updates.
//
// A removed collider's key stays taken until the step after it, whose
// exits name it, has been told: then it's free again.

/// Trigger callbacks (Unity's `OnTriggerEnter` and the rest), for a
/// [GameObject] or a [Behaviour]: `class Coin extends GameObject with
/// Triggers`. They come for a trigger on the object, or on a rigidbody's
/// object for the triggers its colliders touch, with the other collider.
///
/// A trigger meets another collider when at least one of the two is on a
/// rigidbody (kinematic is enough), or is a [CharacterController] that has
/// moved. Enter comes the step they start to
/// overlap, stay every step after while either rigidbody is awake, and exit
/// the step they stop, or after one is disabled, removed, destroyed,
/// hidden, moved to a layer that doesn't meet the other's, or switched
/// between trigger and solid. A hidden or destroyed object gets no exit of
/// its own; the other side does.
///
/// A behaviour gets them while it's disabled too, as in Unity. What one
/// throws goes to [World.errors], and the others are still called.
mixin Triggers {
  /// [other] began to overlap.
  void onTriggerEnter(Collider other) {}

  /// [other] still overlaps, at the end of a step. Not while both sides'
  /// rigidbodies sleep, as Unity's: a kinematic body the game moves by
  /// setting its position wakes, but one left still falls asleep after a
  /// moment, and its stays stop until it moves or [Rigidbody.wakeUp]. To
  /// know what's inside, keep a set from enter and exit instead.
  void onTriggerStay(Collider other) {}

  /// [other] stopped overlapping, or one of the two left.
  void onTriggerExit(Collider other) {}
}

/// Collision callbacks (Unity's `OnCollisionEnter` and the rest), for a
/// [GameObject] or a [Behaviour]: `class Ball extends GameObject with
/// Collisions`. They come for a solid collider on the object, or on a
/// rigidbody's object for its colliders, when one of the two is a
/// dynamic rigidbody, with a [Collision] that says what was hit and how.
///
/// They come when [Triggers]' do, for touching solid colliders; the
/// [Collision] is the same object each time, so read it in the callback
/// and don't keep it. A behaviour gets them while it's disabled too.
mixin Collisions {
  /// A collider began to touch.
  void onCollisionEnter(Collision collision) {}

  /// A collider still touches, at the end of a step. Not while both sides'
  /// rigidbodies sleep, as Unity's (see [Triggers.onTriggerStay]).
  void onCollisionStay(Collision collision) {}

  /// A collider stopped touching, or one of the two left: the collision
  /// has no contacts, impulse or speed.
  void onCollisionExit(Collision collision) {}
}

/// What a collision callback is told (Unity's `Collision`), from its own
/// side: the other collider, its object and rigidbody, and where they
/// touch. Read it in the callback: it's reused for the next.
final class Collision {
  Collision._(this._physics);

  final Physics _physics;
  late Collider _collider;
  late Collider _own;
  var _index = 0;
  var _flipped = false;

  /// The collider hit.
  Collider get collider => _collider;

  /// The collider hit's object.
  GameObject get gameObject => _collider.gameObject;

  /// The rigidbody the collider hit is on, or null for a static one.
  Rigidbody? get rigidbody => _collider._contactBody;

  /// The impulse that pushed this side apart over the step, in N·s: the
  /// force times the step's time.
  Vec3 get impulse => _physics._contactVector(_index, 6, _flipped);

  /// How fast the other side came at this one, in m/s: the other's
  /// velocity less this one's, before the step.
  Vec3 get relativeVelocity => _physics._contactVector(_index, 9, _flipped);

  /// How many points they touch at: none for an exit.
  int get contactCount => _physics._contactUints[_index * _contactWords + 13];

  /// The point at [index], below [contactCount].
  ContactPoint getContact(int index) {
    RangeError.checkValidIndex(index, this, 'index', contactCount);
    final physics = _physics;
    final i = (physics._contactUints[_index * _contactWords + 12] + index) * _pointFloats;
    final f = physics._pointFloats;
    final s = _flipped ? -1.0 : 1.0;
    return ContactPoint._(
      Vec3(f[i], f[i + 1], f[i + 2]),
      Vec3(f[i + 3] * s, f[i + 4] * s, f[i + 5] * s),
      f[i + 6],
      _own,
      _collider,
    );
  }

  /// Every point they touch at. A new list each time.
  List<ContactPoint> get contacts => [for (var i = 0; i < contactCount; i++) getContact(i)];

  @override
  String toString() => 'Collision(with $_collider)';
}

/// Where two colliders touch (Unity's `ContactPoint`).
final class ContactPoint {
  ContactPoint._(this.point, this.normal, this.separation, this.thisCollider, this.otherCollider);

  /// The point, in the world.
  final Vec3 point;

  /// Out of the other collider, towards this side.
  final Vec3 normal;

  /// How far apart they are there, in metres: less than 0 while they're
  /// in each other.
  final double separation;

  /// This side's collider.
  final Collider thisCollider;

  /// The other side's collider.
  final Collider otherCollider;

  @override
  String toString() => 'ContactPoint($point, normal: $normal, separation: $separation)';
}

// An FpContact is 14 32-bit words: the kind, whether it's a trigger's, the
// two keys (64-bit, at words 2 and 4), floats 6–8 the impulse on a and 9–11
// the relative velocity (b less a), then the first point and the count. An
// FpContactPoint is 7 floats: the point, the normal (from b towards a) and
// the separation.
const _contactWords = 14;
const _pointFloats = 7;

/// The layer matrix and the pairs that ignore each other (Unity's
/// `Physics.IgnoreLayerCollision` and `Physics.IgnoreCollision`).
extension PhysicsLayers on Physics {
  /// Makes colliders on layers [layer1] and [layer2] pass through each
  /// other, or ([ignore] false) meet again, from the next step: no
  /// contact, no trigger, no callback. Every layer meets every other
  /// unless set. Queries don't read it: their layer mask says what they
  /// hit.
  void ignoreLayerCollision(int layer1, int layer2, {bool ignore = true}) {
    _checkLayer(layer1, 'layer1');
    _checkLayer(layer2, 'layer2');
    checkPhysics(fp_layer_collision_set(_world, layer1, layer2, ignore ? 0 : 1));
  }

  /// Whether layers [layer1] and [layer2] pass through each other.
  bool getIgnoreLayerCollision(int layer1, int layer2) {
    _checkLayer(layer1, 'layer1');
    _checkLayer(layer2, 'layer2');
    checkPhysics(fp_layer_collision_get(_world, layer1, layer2, _flag));
    return _flag.value == 0;
  }

  /// Makes [collider1] and [collider2] pass through each other, or
  /// ([ignore] false) meet again, from the next step. It holds until
  /// either leaves the world (is disabled, removed or hidden), as in
  /// Unity. Throws a [StateError] if either isn't in the world, and an
  /// [ArgumentError] if they're the same collider.
  void ignoreCollision(Collider collider1, Collider collider2, {bool ignore = true}) {
    _checkHere('ignoreCollision', collider1, collider2);
    if (identical(collider1, collider2)) {
      throw ArgumentError('ignoreCollision was given $collider1 twice: a collider can\'t ignore itself');
    }
    checkPhysics(fp_collision_ignore_set(_world, collider1._id, collider2._id, ignore ? 1 : 0));
  }

  /// Whether [collider1] and [collider2] pass through each other, by
  /// [ignoreCollision]. Throws a [StateError] if either isn't in the world.
  bool getIgnoreCollision(Collider collider1, Collider collider2) {
    _checkHere('getIgnoreCollision', collider1, collider2);
    checkPhysics(fp_collision_ignore_get(_world, collider1._id, collider2._id, _flag));
    return _flag.value == 1;
  }

  /// Throws a [StateError] unless [collider1] and [collider2] are both in
  /// this world's physics.
  void _checkHere(String what, Collider collider1, Collider collider2) {
    for (final c in [collider1, collider2]) {
      if (!identical(c._inWorld(what), this)) {
        throw StateError(
          '$what: $collider1 and $collider2 must both be in this physics\' world, and $c is in another world. '
          'Ask the physics of the world they\'re in (a behaviour\'s own `physics`)',
        );
      }
    }
  }

  static void _checkLayer(int layer, String name) => RangeError.checkValueInInterval(layer, 0, 31, name);
}

/// What [object] listens for, as the native world has it: 1 if it or one
/// of its components is [Triggers], and 2 if one is [Collisions].
int _listensOf(GameObject object) {
  var listens = _listensOfOne(object);
  final count = object.componentCount;
  for (var i = 0; i < count; i++) {
    final c = object.componentAt(i);
    if (!c.isRemoved) listens |= _listensOfOne(c);
  }
  return listens;
}

int _listensOfOne(Object o) => (o is Triggers ? 1 : 0) | (o is Collisions ? 2 : 0);
