// Typed engine events decoded from the event buffer (pull-only, ADR-0008).

import 'dart:typed_data';

import '../generated/schema.g.dart' as gen;
import '../math.dart';
import 'wire/buffers.dart';

sealed class EngineEvent {
  const EngineEvent(this.tickLo);

  /// Low 32 bits of the tick that produced the event.
  final int tickLo;
}

class EntitySpawned extends EngineEvent {
  const EntitySpawned(super.tickLo, this.tempId, this.entity);
  final int tempId, entity;
}

class EntityDestroyed extends EngineEvent {
  const EntityDestroyed(super.tickLo, this.entity);
  final int entity;
}

class ParentChanged extends EngineEvent {
  const ParentChanged(super.tickLo, this.child, this.parent);
  final int child;
  final int? parent;
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
  final int zone, other;
  final String event;
}

class ZoneEntered extends ZoneEvent {
  const ZoneEntered(super.tickLo, super.zone, super.other, super.event);

  @override
  String toString() => 'ZoneEntered($event zone=${zone.toRadixString(16)} other=${other.toRadixString(16)})';
}

class ZoneExited extends ZoneEvent {
  const ZoneExited(super.tickLo, super.zone, super.other, super.event);

  @override
  String toString() => 'ZoneExited($event zone=${zone.toRadixString(16)} other=${other.toRadixString(16)})';
}

enum ContactKind { collisionEnter, collisionStay, collisionExit, triggerEnter, triggerExit }

/// A collision or trigger event between two colliders (M3-08). For
/// collisions, [point] is the deepest contact point (world space), [normal]
/// points from [a] towards [b] and [impulse] is the impulse the solver
/// applied at this pair (N·s; 0 on exit). Triggers carry no contact
/// geometry (zeros).
class Contact extends EngineEvent {
  const Contact(super.tickLo, this.kind, this.a, this.b,
      {this.point = Vec3.zero, this.normal = Vec3.zero, this.impulse = 0});
  final ContactKind kind;
  final int a, b;
  final Vec3 point;
  final Vec3 normal;
  final double impulse;

  bool get isTrigger => kind == ContactKind.triggerEnter || kind == ContactKind.triggerExit;

  bool involves(int entity) => a == entity || b == entity;

  /// The other side seen from [entity] (which must be [a] or [b]).
  int other(int entity) {
    if (entity == a) return b;
    if (entity == b) return a;
    throw ArgumentError.value(entity, 'entity', 'not a side of this contact');
  }

  /// [normal] as seen from [entity]: pointing away from it.
  Vec3 normalFor(int entity) => entity == a ? normal : Vec3(-normal.x, -normal.y, -normal.z);

  @override
  String toString() => 'Contact($kind ${a.toRadixString(16)}↔${b.toRadixString(16)} at $point n=$normal j=$impulse)';
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
      gen.SpawnedEvent.eventKind => _spawned(d, body, tail, h.tickLo),
      gen.DestroyedEvent.eventKind => EntityDestroyed(h.tickLo, gen.DestroyedEvent.decode(d, body, tail).entity),
      gen.ParentChangedEvent.eventKind => _parent(d, body, tail, h.tickLo),
      gen.MissedDeadlineEvent.eventKind =>
        MissedDeadline(h.tickLo, gen.MissedDeadlineEvent.decode(d, body, tail).droppedSeconds),
      gen.SceneReadyEvent.eventKind => SceneReady(h.tickLo, scene: gen.SceneReadyEvent.decode(d, body, tail).scene.asset),
      gen.SceneUnloadedEvent.eventKind => _unloaded(d, body, tail, h.tickLo),
      gen.ZoneEvent.eventKind => _zone(d, body, tail, h.tickLo, entered: true),
      _zoneExitedKind => _zone(d, body, tail, h.tickLo, entered: false),
      >= _physicsFirst && < _physicsFirst + 5 => _contact(d, body, tail, h),
      _ => UnknownEvent(h.tickLo, h.kind),
    };
  }
}

EntitySpawned _spawned(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.SpawnedEvent.decode(d, o, tail);
  return EntitySpawned(tickLo, e.tempId, e.entity);
}

SceneUnloaded _unloaded(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.SceneUnloadedEvent.decode(d, o, tail);
  return SceneUnloaded(tickLo, e.scene.asset, e.entities);
}

ZoneEvent _zone(ByteData d, int o, Uint8List tail, int tickLo, {required bool entered}) {
  final e = gen.ZoneEvent.decode(d, o, tail);
  return entered ? ZoneEntered(tickLo, e.zone, e.other, e.event) : ZoneExited(tickLo, e.zone, e.other, e.event);
}

ParentChanged _parent(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.ParentChangedEvent.decode(d, o, tail);
  return ParentChanged(tickLo, e.child, e.parent);
}

Contact _contact(ByteData d, int o, Uint8List tail, gen.EventHeader h) {
  final e = gen.ContactEvent.decode(d, o, tail);
  return Contact(h.tickLo, ContactKind.values[h.kind - _physicsFirst], e.a, e.b,
      point: e.point, normal: e.normal, impulse: e.impulse);
}
