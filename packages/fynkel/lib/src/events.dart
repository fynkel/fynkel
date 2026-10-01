// Typed engine events decoded from the event buffer (pull-only, ADR-0008).

import 'dart:typed_data';

import 'entity_ref.dart';

import '../generated/schema.g.dart' as gen;
import '../math.dart';
import 'asset_ref.dart';
import 'wire/buffers.dart';

sealed class EngineEvent {
  const EngineEvent(this.tickLo);

  /// Low 32 bits of the tick that produced the event.
  final int tickLo;
}

class EntityDestroyed extends EngineEvent {
  const EntityDestroyed(super.tickLo, this.entity);
  final EntityRef entity;
}

class ParentChanged extends EngineEvent {
  const ParentChanged(super.tickLo, this.child, this.parent);
  final EntityRef child;
  final EntityRef? parent;
}

class MissedDeadline extends EngineEvent {
  const MissedDeadline(super.tickLo, this.droppedSeconds);
  final double droppedSeconds;
}

/// A scene finished loading; [scene] is its asset id (M3-12).
class SceneReady extends EngineEvent {
  const SceneReady(super.tickLo, {this.scene = ''});
  final String scene;
}

/// A loaded scene was unloaded (M3-12): [scene] is its asset id,
/// [entities] how many of its entities went (their `EntityDestroyed`
/// events precede this one).
class SceneUnloaded extends EngineEvent {
  const SceneUnloaded(super.tickLo, this.scene, this.entities);
  final String scene;
  final int entities;
}

/// Something entered or left a `fynkel.TriggerZone`'s sensor (M3-12):
/// [zone] is the zone's entity, [other] what came in, [event] the
/// zone's named event (`"collect"`). General lane — behaviours see it in
/// `onZoneEnter`/`onZoneExit`, gameplay in `world.zones`.
sealed class ZoneEvent extends EngineEvent {
  const ZoneEvent(super.tickLo, this.zone, this.other, this.event);
  final EntityRef zone, other;
  final String event;
}

class ZoneEntered extends ZoneEvent {
  const ZoneEntered(super.tickLo, super.zone, super.other, super.event);

  @override
  String toString() => 'ZoneEntered($event zone=${zone.toString()} other=${other.toString()})';
}

class ZoneExited extends ZoneEvent {
  const ZoneExited(super.tickLo, super.zone, super.other, super.event);

  @override
  String toString() => 'ZoneExited($event zone=${zone.toString()} other=${other.toString()})';
}

/// A `fynkel.Animator`'s non-looping clip reached its end (M3-19):
/// [entity] is the animator's, [clip] the clip. General lane, once per
/// play; behaviours see it in `onAnimationEnded`.
class AnimationEnded extends EngineEvent {
  const AnimationEnded(super.tickLo, this.entity, this.clip);
  final EntityRef entity;
  final AssetRef clip;

  @override
  String toString() => 'AnimationEnded(${entity.toString()} $clip)';
}

/// A `fynkel.AudioSource`'s non-looping clip reached its end (M3-11):
/// [entity] is the source's, [clip] the clip. General lane, once per
/// play; `stop`, a stolen voice and a looping clip never raise it;
/// behaviours see it in `onPlaybackEnded`.
class PlaybackEnded extends EngineEvent {
  const PlaybackEnded(super.tickLo, this.entity, this.clip);
  final EntityRef entity;
  final AssetRef clip;

  @override
  String toString() => 'PlaybackEnded(${entity.toString()} $clip)';
}

/// A `fynkel.Animator`'s clip crossed a marker (M3-19c): [entity] is the
/// animator's, [clip] the clip carrying the marker, [name] the marker's.
/// Once per crossing, loops and backwards playback included; behaviours
/// see it in `onAnimationMarker`.
class AnimationMarker extends EngineEvent {
  const AnimationMarker(super.tickLo, this.entity, this.clip, this.name);
  final EntityRef entity;
  final AssetRef clip;
  final String name;

  @override
  String toString() => 'AnimationMarker(${entity.toString()} $clip "$name")';
}

/// A layer of a `fynkel.Animator`'s controller entered or left a state
/// (ADR-0032): [entity] is the animator's, [layer] the layer's index,
/// [state] the state's name.
sealed class AnimatorStateEvent extends EngineEvent {
  const AnimatorStateEvent(super.tickLo, this.entity, this.layer, this.state);
  final EntityRef entity;
  final int layer;
  final String state;
}

/// A state started: at the start of the crossfade into it, or at once
/// for a jump. [from] is the state left, empty on the first tick.
/// Behaviours see it in `onAnimatorStateEntered` (Unity's
/// `OnStateEnter`).
class AnimatorStateEntered extends AnimatorStateEvent {
  const AnimatorStateEntered(super.tickLo, super.entity, super.layer, super.state, this.from);
  final String from;

  @override
  String toString() => 'AnimatorStateEntered(${entity.toString()} $layer: $from → $state)';
}

/// A state stopped playing: at the end of the crossfade out of it, or at
/// once for a jump. Behaviours see it in `onAnimatorStateExited`
/// (Unity's `OnStateExit`).
class AnimatorStateExited extends AnimatorStateEvent {
  const AnimatorStateExited(super.tickLo, super.entity, super.layer, super.state);

  @override
  String toString() => 'AnimatorStateExited(${entity.toString()} $layer: $state)';
}

/// In wire order: kind `0x100 + index` (`triggerStay` is 0x105, foundation 2.3).
enum ContactKind {
  collisionEnter,
  collisionStay,
  collisionExit,
  triggerEnter,
  triggerExit,
  triggerStay,

  /// A character's move touched a collider (Unity's
  /// `OnControllerColliderHit`, foundation 2.4): [Contact.a] the character,
  /// [Contact.b] the collider, [Contact.bodyB] its body, [Contact.normal]
  /// from the character into it, no impulse.
  controllerHit,
}

/// One point of a [Contact] (Unity's `ContactPoint`): [point] in world
/// space, [normal] from the contact's `a` towards `b`, [separation] the
/// distance between the shapes there (negative when they overlap),
/// [impulse] the solver's push along the normal at this point (N·s).
class ContactPoint {
  const ContactPoint(this.point, this.normal, this.separation, this.impulse);
  final Vec3 point;
  final Vec3 normal;
  final double separation;
  final double impulse;

  @override
  bool operator ==(Object other) =>
      other is ContactPoint &&
      other.point == point &&
      other.normal == normal &&
      other.separation == separation &&
      other.impulse == impulse;

  @override
  int get hashCode => Object.hash(point, normal, separation, impulse);

  @override
  String toString() => 'ContactPoint($point n=$normal d=$separation j=$impulse)';
}

/// A collision or trigger event between two colliders (M3-08), Unity's
/// `Collision`. For collisions, [contacts] are the pair's points (the
/// deepest first, at most four), [point] and [normal] the deepest one's,
/// the normal pointing from [a] towards [b]; [impulse] the impulse the
/// solver applied to [a] over the step (N·s; `impulseFor` gives either
/// side's) and [relativeVelocity] `b`'s velocity minus `a`'s — at an
/// enter, the speed of the impact before the solver stopped it
/// (`relativeVelocityFor(me).length > 2` plays the thud; foundation 2.9).
/// Exits carry no points and no impulse; triggers carry nothing but the
/// sides.
class Contact extends EngineEvent {
  const Contact(
    super.tickLo,
    this.kind,
    this.a,
    this.b, {
    this.point = Vec3.zero,
    this.normal = Vec3.zero,
    this.impulse = Vec3.zero,
    this.relativeVelocity = Vec3.zero,
    this.contacts = const [],
    this.bodyA,
    this.bodyB,
  });
  final ContactKind kind;

  /// The colliders' entities (Unity's `Collision.collider`): for a collider
  /// on a child of a Rigidbody, the child.
  final EntityRef a, b;
  final Vec3 point;
  final Vec3 normal;
  final Vec3 impulse;
  final Vec3 relativeVelocity;
  final List<ContactPoint> contacts;

  /// The entity whose `RigidBody` each collider belongs to (Unity's
  /// `Collision.rigidbody`): the collider's own, or its parent's when it
  /// joined it (ADR-0034); null for static geometry and characters.
  final EntityRef? bodyA, bodyB;

  /// Unity's `Collision.contactCount`.
  int get contactCount => contacts.length;

  /// Unity's `Collision.GetContact`.
  ContactPoint getContact(int index) => contacts[index];

  /// The impulse the side [entity] (a collider or its body) received.
  Vec3 impulseFor(EntityRef entity) => _isA(entity) ? impulse : -impulse;

  /// The other side's velocity minus [entity]'s (Unity's
  /// `Collision.relativeVelocity` as that side hears it).
  Vec3 relativeVelocityFor(EntityRef entity) => _isA(entity) ? relativeVelocity : -relativeVelocity;

  bool get isTrigger =>
      kind == ContactKind.triggerEnter || kind == ContactKind.triggerExit || kind == ContactKind.triggerStay;

  /// Whether [entity] is a side of this contact: a collider or its body.
  bool involves(EntityRef entity) => entity == a || entity == b || entity == bodyA || entity == bodyB;

  /// The body of the side [entity] is on (a collider or a body), null for
  /// static geometry.
  EntityRef? bodyOf(EntityRef entity) => _isA(entity) ? bodyA : bodyB;

  /// The other side's collider seen from [entity] — a collider of this
  /// contact or a body one belongs to.
  EntityRef other(EntityRef entity) => _isA(entity) ? b : a;

  /// [normal] as seen from [entity] (a collider or its body): pointing away
  /// from it.
  Vec3 normalFor(EntityRef entity) => _isA(entity) ? normal : Vec3(-normal.x, -normal.y, -normal.z);

  bool _isA(EntityRef entity) {
    if (entity == a || entity == bodyA) return true;
    if (entity == b || entity == bodyB) return false;
    throw ArgumentError.value(entity, 'entity', 'not a side of this contact');
  }

  @override
  String toString() =>
      'Contact($kind ${a.toString()}↔${b.toString()} at $point n=$normal j=$impulse '
      'v=$relativeVelocity points=$contactCount)';
}

/// An event whose kind this SDK does not know (newer engine); skipped safely.
class UnknownEvent extends EngineEvent {
  const UnknownEvent(super.tickLo, this.kind);
  final int kind;
}

const int _physicsFirst = 0x100;

/// `ZONE_EXITED` shares `fynkel.ev.Zone`'s body with `ZONE_ENTERED` (kind 7).
const int _zoneExitedKind = gen.ZoneEvent.eventKind + 1;

Iterable<EngineEvent> decodeEvents(WireBuffer b) sync* {
  final d = b.records;
  var off = 0;
  for (var i = 0; i < b.recordCount; i++) {
    final h = gen.EventHeader.decode(d, off, b.tail);
    off += gen.EventHeader.wireSize;
    final body = off;
    off += h.size;
    final tail = b.tail;
    yield switch (h.kind) {
      gen.DestroyedEvent.eventKind => EntityDestroyed(h.tickLo, gen.DestroyedEvent.decode(d, body, tail).entity),
      gen.ParentChangedEvent.eventKind => _parent(d, body, tail, h.tickLo),
      gen.MissedDeadlineEvent.eventKind => MissedDeadline(
        h.tickLo,
        gen.MissedDeadlineEvent.decode(d, body, tail).droppedSeconds,
      ),
      gen.SceneReadyEvent.eventKind => SceneReady(
        h.tickLo,
        scene: gen.SceneReadyEvent.decode(d, body, tail).scene.asset,
      ),
      gen.SceneUnloadedEvent.eventKind => _unloaded(d, body, tail, h.tickLo),
      gen.ZoneEvent.eventKind => _zone(d, body, tail, h.tickLo, entered: true),
      _zoneExitedKind => _zone(d, body, tail, h.tickLo, entered: false),
      gen.AnimationEndedEvent.eventKind => _animationEnded(d, body, tail, h.tickLo),
      gen.AnimationMarkerEvent.eventKind => _animationMarker(d, body, tail, h.tickLo),
      gen.PlaybackEndedEvent.eventKind => _playbackEnded(d, body, tail, h.tickLo),
      gen.AnimatorStateEnteredEvent.eventKind => _stateEntered(d, body, tail, h.tickLo),
      gen.AnimatorStateExitedEvent.eventKind => _stateExited(d, body, tail, h.tickLo),
      >= _physicsFirst && < _physicsFirst + 7 => _contact(d, body, tail, h),
      _ => UnknownEvent(h.tickLo, h.kind),
    };
  }
}

SceneUnloaded _unloaded(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.SceneUnloadedEvent.decode(d, o, tail);
  return SceneUnloaded(tickLo, e.scene.asset, e.entities);
}

ZoneEvent _zone(ByteData d, int o, Uint8List tail, int tickLo, {required bool entered}) {
  final e = gen.ZoneEvent.decode(d, o, tail);
  return entered ? ZoneEntered(tickLo, e.zone, e.other, e.event) : ZoneExited(tickLo, e.zone, e.other, e.event);
}

AnimationEnded _animationEnded(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.AnimationEndedEvent.decode(d, o, tail);
  return AnimationEnded(tickLo, e.entity, e.clip);
}

AnimatorStateEntered _stateEntered(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.AnimatorStateEnteredEvent.decode(d, o, tail);
  return AnimatorStateEntered(tickLo, e.entity, e.layer, e.state, e.from);
}

AnimatorStateExited _stateExited(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.AnimatorStateExitedEvent.decode(d, o, tail);
  return AnimatorStateExited(tickLo, e.entity, e.layer, e.state);
}

AnimationMarker _animationMarker(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.AnimationMarkerEvent.decode(d, o, tail);
  return AnimationMarker(tickLo, e.entity, e.clip, e.name);
}

PlaybackEnded _playbackEnded(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.PlaybackEndedEvent.decode(d, o, tail);
  return PlaybackEnded(tickLo, e.entity, e.clip);
}

ParentChanged _parent(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.ParentChangedEvent.decode(d, o, tail);
  return ParentChanged(tickLo, e.child, e.parent);
}

Contact _contact(ByteData d, int o, Uint8List tail, gen.EventHeader h) {
  final e = gen.ContactEvent.decode(d, o, tail);
  final points = [
    e.contact0,
    e.contact1,
    e.contact2,
    e.contact3,
  ].take(e.contactCount).map((p) => ContactPoint(p.point, p.normal, p.separation, p.impulse)).toList(growable: false);
  return Contact(
    h.tickLo,
    ContactKind.values[h.kind - _physicsFirst],
    e.a,
    e.b,
    point: e.point,
    normal: e.normal,
    impulse: e.impulse,
    relativeVelocity: e.relativeVelocity,
    contacts: points,
    bodyA: e.bodyA,
    bodyB: e.bodyB,
  );
}
