// Typed engine events decoded from the event buffer (pull-only, ADR-0008).

import 'dart:typed_data';

import '../generated/schema.g.dart' as gen;
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

class SceneReady extends EngineEvent {
  const SceneReady(super.tickLo);
}

enum ContactKind { collisionEnter, collisionStay, collisionExit, triggerEnter, triggerExit }

class Contact extends EngineEvent {
  const Contact(super.tickLo, this.kind, this.a, this.b);
  final ContactKind kind;
  final int a, b;
}

/// An event whose kind this SDK does not know (newer engine); skipped safely.
class UnknownEvent extends EngineEvent {
  const UnknownEvent(super.tickLo, this.kind);
  final int kind;
}

const int _physicsFirst = 0x100;

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
      gen.SceneReadyEvent.eventKind => SceneReady(h.tickLo),
      >= _physicsFirst && < _physicsFirst + 5 => _contact(d, body, tail, h),
      _ => UnknownEvent(h.tickLo, h.kind),
    };
  }
}

EntitySpawned _spawned(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.SpawnedEvent.decode(d, o, tail);
  return EntitySpawned(tickLo, e.tempId, e.entity);
}

ParentChanged _parent(ByteData d, int o, Uint8List tail, int tickLo) {
  final e = gen.ParentChangedEvent.decode(d, o, tail);
  return ParentChanged(tickLo, e.child, e.parent);
}

Contact _contact(ByteData d, int o, Uint8List tail, gen.EventHeader h) {
  final e = gen.ContactEvent.decode(d, o, tail);
  return Contact(h.tickLo, ContactKind.values[h.kind - _physicsFirst], e.a, e.b);
}
