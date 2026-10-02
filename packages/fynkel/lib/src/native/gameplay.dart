// Internal F2 bridge protocol. This is not the final ordinary gameplay API.
part of 'bridge.dart';

class NativeGameplayRef {
  NativeGameplayRef.live(EntityRef entity)
    : _entity = entity,
      _world = entity.world,
      _sequence = 0,
      _index = 0;
  const NativeGameplayRef._pending(this._world, this._sequence, this._index)
    : _entity = null;
  final EntityRef? _entity;
  final int _world, _sequence, _index;
  EntityRef? get live => _entity;
  bool get isPending => _entity == null;
  @override
  bool operator ==(Object other) =>
      other is NativeGameplayRef &&
      _entity == other._entity &&
      _world == other._world &&
      _sequence == other._sequence &&
      _index == other._index;
  @override
  int get hashCode => Object.hash(_entity, _world, _sequence, _index);
}

class NativeGameplayCommit<R> {
  NativeGameplayCommit._(this.value, this._edit, this._resolved);
  final R value;
  final NativeGameplayEdit _edit;
  final Map<int, EntityRef> _resolved;
  EntityRef? resolve(NativeGameplayRef reference) {
    if (!reference.isPending) return reference.live;
    if (reference._sequence != _edit._sequence ||
        reference._world != _edit._worldId) {
      return null;
    }
    return _resolved[reference._index];
  }
}

/// Generated record plus tagged references in catalogue field order. Without
/// an object resolver, reference fields in [value] are null. The SDK adapter
/// supplies a resolver to reconstruct ordinary GameObject references.
class NativeGameplayComponent<T extends WireRecord> {
  NativeGameplayComponent(this.value, List<NativeGameplayRef?> references)
    : references = List.unmodifiable(references);
  final T value;
  final List<NativeGameplayRef?> references;
}

class NativeGameplayPrefab {
  NativeGameplayPrefab(this.root, this.entities, this.metadata);
  final NativeGameplayRef root;
  final Map<String, NativeGameplayRef> entities;
  final Map<String, Object?> metadata;
}

class NativeGameplayClone {
  NativeGameplayClone(
    this.root,
    Map<NativeGameplayRef, NativeGameplayRef> entities,
  ) : entities = Map.unmodifiable(entities);
  final NativeGameplayRef root;
  final Map<NativeGameplayRef, NativeGameplayRef> entities;
}

class NativeRegisteredValue {
  const NativeRegisteredValue(this.kind, this.value);
  final int kind;
  final Object? value;
}

class NativeRegisteredField {
  const NativeRegisteredField(this.name, this.defaultValue);
  final String name;
  final NativeRegisteredValue defaultValue;
}

class NativeRegisteredBatch {
  NativeRegisteredBatch._(this._edit, this._token, this.rows);
  final NativeGameplayEdit _edit;
  final int _token;
  final List<(NativeGameplayRef, List<NativeRegisteredValue>)> rows;
}

class NativeGameplayEdit {
  NativeGameplayEdit._(this._native, this._sequence);
  final NativeWorld _native;
  final int _sequence;
  int? _worldId;
  bool _closed = false, _failed = false;
  T _guard<T>(T Function() f) {
    if (_closed) throw StateError('gameplay edit has expired');
    if (_failed) throw StateError('gameplay edit previously failed');
    try {
      return f();
    } catch (_) {
      _failed = true;
      rethrow;
    }
  }

  List<NativeRegisteredValue>? registeredGet(
    NativeGameplayRef owner,
    String id,
  ) => _guard(() {
    final r = _send(
      _GameplayWriter(35)
        ..reference(owner)
        ..string(id),
    );
    return r.u32() == 0 ? null : r.scalars();
  });
  void registeredSet(
    NativeGameplayRef owner,
    String id,
    List<NativeRegisteredValue> values,
  ) => _guard(() {
    _send(
      _GameplayWriter(36)
        ..reference(owner)
        ..string(id)
        ..scalars(values),
    );
  });
  void registeredRemove(NativeGameplayRef owner, String id) => _guard(() {
    _send(
      _GameplayWriter(37)
        ..reference(owner)
        ..string(id),
    );
  });
  NativeRegisteredBatch registeredQuery(String id) => _guard(() {
    final r = _send(_GameplayWriter(38)..string(id));
    final token = r.u32(), count = r.u32();
    return NativeRegisteredBatch._(
      this,
      token,
      List.unmodifiable(
        List.generate(
          count,
          (_) =>
              (_ref(r)!, List<NativeRegisteredValue>.unmodifiable(r.scalars())),
        ),
      ),
    );
  });
  void registeredApply(
    NativeRegisteredBatch batch,
    List<List<NativeRegisteredValue>> rows,
  ) => _guard(() {
    if (!identical(batch._edit, this)) {
      throw StateError('bulk view belongs to another edit');
    }
    final w = _GameplayWriter(39)
      ..u32(batch._token)
      ..u32(rows.length);
    for (final row in rows) {
      w.scalars(row);
    }
    _send(w);
  });

  bool persist(NativeGameplayRef owner) => _guard(() {
    final writer = _GameplayWriter(33)..reference(owner);
    return _send(writer).u32() != 0;
  });
  List<NativeGameplayRef> persistentRoots() => _guard(() {
    final reader = _send(_GameplayWriter(34));
    return List.generate(reader.u32(), (_) => _ref(reader)!);
  });

  Map<NativeGameplayRef, String?> identityChanges() => _guard(() {
    final reader = _send(_GameplayWriter(32));
    final result = <NativeGameplayRef, String?>{};
    final length = reader.u32();
    for (var i = 0; i < length; i++) {
      final owner = _ref(reader)!;
      result[owner] = reader.u32() == 0 ? null : reader.string();
    }
    return result;
  });

  void setRuntimeMaterial(String id, gen.PbrMaterial? material) =>
      _runtimeResource(
        29,
        id,
        material == null ? null : NativeWorld.materialPayload(material),
      );
  void setRuntimeMesh(String id, Uint8List? payload) =>
      _runtimeResource(30, id, payload);
  void setRuntimeShaderMaterial(String id, String? document) =>
      _runtimeResource(
        31,
        id,
        document == null ? null : Uint8List.fromList(utf8.encode(document)),
      );
  void _runtimeResource(int operation, String id, Uint8List? payload) =>
      _guard(() {
        final writer = _GameplayWriter(operation)
          ..string(id)
          ..u32(payload?.length ?? 0);
        if (payload != null) writer.bytes.add(payload);
        _send(writer);
      });

  void setRuntimeClip(String id, Uint8List? payload) => _guard(() {
    final writer = _GameplayWriter(28)
      ..string(id)
      ..u32(payload?.length ?? 0);
    if (payload != null) writer.bytes.add(payload);
    _send(writer);
  });

  _GameplayReader _send(_GameplayWriter writer) => _guard(() {
    final bytes = writer.bytes.takeBytes();
    _native._toScratch(bytes, bytes.length);
    final status = fynkel_world_gameplay_exchange(
      _native._w,
      _sequence,
      _native._scratch,
      bytes.length,
      _native._view,
    );
    _native.ffiCalls++;
    final result = Uint8List.fromList(
      _native._view.ref.data.asTypedList(_native._view.ref.length),
    );
    _native.dartCopies++;
    _native.dartBytesCopied += result.length;
    if (status != FynkelStatus.FYNKEL_STATUS_OK) {
      throw StateError(utf8.decode(result));
    }
    return _GameplayReader(result);
  });
  NativeGameplayRef? _ref(_GameplayReader reader) {
    final r = reader.reference();
    if (r != null && r.isPending) _worldId = r._world;
    return r;
  }

  NativeGameplayRef spawn({
    gen.Transform? transform,
    NativeGameplayRef? parent,
    String name = '',
  }) => _guard(() {
    final w = _GameplayWriter(1);
    final t = transform ?? const gen.Transform();
    final body = ByteData(gen.Transform.wireSize);
    t.encode(body, 0, WireStrings());
    w.bytes.add(body.buffer.asUint8List());
    w.reference(parent);
    w.string(name);
    return _ref(_send(w))!;
  });
  void playSound(gen.PlaySoundCommandRecord command) => _guard(() {
    if (command.priority < -2147483648 || command.priority > 2147483647) {
      throw ArgumentError.value(
        command.priority,
        'priority',
        'must fit a signed 32-bit integer',
      );
    }
    final body = ByteData(gen.PlaySoundCommandRecord.wireSize),
        strings = WireStrings();
    command.encode(body, 0, strings);
    final tail = strings.takeBytes();
    final w = _GameplayWriter(20);
    w.bytes.add(body.buffer.asUint8List());
    w.u32(tail.length);
    w.bytes.add(tail);
    _send(w);
  });
  (gen.AnimatorParameterKind, Object)? controllerParameter(
    NativeGameplayRef owner,
    String name,
  ) => _guard(() {
    final w = _GameplayWriter(23)
      ..reference(owner)
      ..string(name);
    final r = _send(w), kind = r.u32();
    if (kind == 0) return null;
    final bits = r.u32();
    final data = ByteData(4)..setUint32(0, bits, Endian.little);
    return switch (kind) {
      1 => (gen.AnimatorParameterKind.float, data.getFloat32(0, Endian.little)),
      2 => (gen.AnimatorParameterKind.int, data.getInt32(0, Endian.little)),
      3 => (gen.AnimatorParameterKind.bool, bits != 0),
      4 => (gen.AnimatorParameterKind.trigger, bits != 0),
      _ => throw StateError('invalid controller parameter kind'),
    };
  });
  void animateControl(
    NativeGameplayRef owner,
    gen.AnimateCommandRecord record,
  ) => _guard(() {
    if ((record.op == gen.AnimateOp.playState ||
            record.op == gen.AnimateOp.crossFadeState) &&
        (record.layer < -1 || record.layer > 2147483647)) {
      throw ArgumentError.value(
        record.layer,
        'layer',
        'must be -1 or a nonnegative signed 32-bit index',
      );
    }
    if (record.op == gen.AnimateOp.setInteger &&
        (record.integer < -2147483648 || record.integer > 2147483647)) {
      throw ArgumentError.value(
        record.integer,
        'integer',
        'must fit signed 32 bits',
      );
    }
    final w = _GameplayWriter(22)..reference(owner);
    final body = ByteData(gen.AnimateCommandRecord.wireSize),
        strings = WireStrings();
    record.encode(body, 0, strings);
    final tail = strings.takeBytes();
    w.bytes.add(body.buffer.asUint8List());
    w.u32(tail.length);
    w.bytes.add(tail);
    _send(w);
  });
  void audioControl(NativeGameplayRef owner, gen.AudioOp op, double value) =>
      _guard(() {
        final w = _GameplayWriter(21)..reference(owner);
        final record = gen.AudioCommandRecord(
          entity: EntityRef.nil,
          op: op,
          value: value,
        );
        final body = ByteData(gen.AudioCommandRecord.wireSize);
        record.encode(body, 0, WireStrings());
        w.bytes.add(body.buffer.asUint8List());
        _send(w);
      });
  void bodyEffect(
    NativeGameplayRef owner,
    gen.BodyOp op,
    Vec3 vector,
    Quat rotation,
  ) => _guard(() {
    final w = _GameplayWriter(19)..reference(owner);
    final command = gen.BodyCommandRecord(
      entity: EntityRef.nil,
      op: op,
      vector: vector,
      rotation: rotation,
    );
    final body = ByteData(gen.BodyCommandRecord.wireSize);
    command.encode(body, 0, WireStrings());
    w.bytes.add(body.buffer.asUint8List());
    _send(w);
  });
  Map<String, Object?> bindActions(Map<String, List<String>> rebinds) =>
      _guard(() {
        final reply = _send(_GameplayWriter(27)..string(jsonEncode(rebinds)));
        return (jsonDecode(reply.string()) as Map).cast<String, Object?>();
      });

  void setAudioBus(gen.AudioBus bus, double volume, {bool muted = false}) =>
      _guard(() {
        final w = _GameplayWriter(25)..u32(bus.index);
        final gain = ByteData(4)..setFloat32(0, volume, Endian.little);
        w.bytes.add(gain.buffer.asUint8List());
        w.u32(muted ? 1 : 0);
        _send(w);
      });

  (double, bool) audioBus(gen.AudioBus bus) => _guard(() {
    final reply = _send(_GameplayWriter(26)..u32(bus.index));
    return (reply.f32(), reply.u32() != 0);
  });

  NativeGameplayClone cloneTree(
    NativeGameplayRef source, {
    NativeGameplayRef? parent,
    gen.Transform? transform,
    String? name,
  }) => _guard(() {
    final w = _GameplayWriter(24)
      ..reference(source)
      ..reference(parent);
    w.u32(transform == null ? 0 : 1);
    if (transform != null) {
      final body = ByteData(gen.Transform.wireSize);
      transform.encode(body, 0, WireStrings());
      w.bytes.add(body.buffer.asUint8List());
    }
    w.u32(name == null ? 0 : 1);
    if (name != null) w.string(name);
    final r = _send(w);
    final root = _ref(r)!;
    final count = r.u32();
    final entities = <NativeGameplayRef, NativeGameplayRef>{};
    for (var i = 0; i < count; i++) {
      final original = _ref(r)!;
      entities[original] = _ref(r)!;
    }
    return NativeGameplayClone(root, entities);
  });

  NativeGameplayPrefab instantiate(
    String id,
    gen.Transform transform,
    NativeGameplayRef? parent,
    String name,
  ) => _guard(() {
    final w = _GameplayWriter(17)..string(id);
    final body = ByteData(gen.Transform.wireSize);
    transform.encode(body, 0, WireStrings());
    w.bytes.add(body.buffer.asUint8List());
    w.reference(parent);
    w.string(name);
    final r = _send(w);
    final root = _ref(r)!;
    final count = r.u32();
    final entities = <String, NativeGameplayRef>{};
    for (var i = 0; i < count; i++) {
      entities[r.string()] = _ref(r)!;
    }
    final metadata = (jsonDecode(r.string()) as Map).cast<String, Object?>();
    return NativeGameplayPrefab(root, entities, metadata);
  });

  Map<String, Object?> assetJson(String id) => _guard(() {
    final reply = _send(_GameplayWriter(18)..string(id));
    return (jsonDecode(reply.string()) as Map).cast<String, Object?>();
  });

  void destroy(NativeGameplayRef entity) =>
      _send(_GameplayWriter(2)..reference(entity));
  void set<T extends WireRecord>(
    NativeGameplayRef entity,
    T value, {
    List<NativeGameplayRef?> references = const [],
    NativeGameplayRef? Function(GameObject?)? encodeReference,
  }) => _guard(() {
    final id = gen.componentIds[value.runtimeType];
    if (id == null) {
      throw ArgumentError('unregistered component ${value.runtimeType}');
    }
    final codec = gen.componentCodecs[id]!;
    final body = ByteData(codec.wireSize), strings = WireStrings();
    if (encodeReference != null && references.isNotEmpty) {
      throw ArgumentError(
        'provide an object resolver or tagged references, not both',
      );
    }
    final encodedReferences = encodeReference == null
        ? references
        : <NativeGameplayRef?>[];
    final scalar = codec.mapObjectReferences(value, (object) {
      if (encodeReference == null) {
        if (object != null) {
          throw ArgumentError('GameObject references require encodeReference');
        }
      } else {
        encodedReferences.add(encodeReference(object));
      }
      return null;
    });
    scalar.encode(body, 0, strings);
    final tail = strings.takeBytes();
    final w = _GameplayWriter(3)
      ..reference(entity)
      ..u32(id)
      ..u32(body.lengthInBytes);
    w.bytes.add(body.buffer.asUint8List());
    w.u32(tail.length);
    w.bytes.add(tail);
    w.u32(encodedReferences.length);
    for (final r in encodedReferences) {
      w.reference(r);
    }
    _send(w);
  });
  NativeGameplayComponent<T>? component<T extends WireRecord>(
    NativeGameplayRef entity, {
    GameObject? Function(NativeGameplayRef?)? decodeReference,
  }) => _guard(() {
    final id = gen.componentIds[T];
    if (id == null) throw ArgumentError('unregistered component $T');
    final r = _send(
      _GameplayWriter(4)
        ..reference(entity)
        ..u32(id),
    );
    if (r.u32() == 0) return null;
    final body = r.take(r.u32()), tail = r.take(r.u32());
    final refs = <NativeGameplayRef?>[];
    final count = r.u32();
    for (var i = 0; i < count; i++) {
      refs.add(_ref(r));
    }
    final codec = gen.componentCodecs[id]!;
    final scalar = codec.decode(ByteData.sublistView(body), 0, tail) as T;
    var index = 0;
    final value = codec.mapObjectReferences(scalar, (_) {
      if (index >= refs.length) {
        throw const FormatException('missing gameplay component reference');
      }
      final reference = refs[index++];
      return decodeReference?.call(reference);
    }) as T;
    if (index != refs.length) {
      throw const FormatException('extra gameplay component references');
    }
    return NativeGameplayComponent(value, refs);
  });

  /// Materialized membership; a later call sees additions/removals.
  List<NativeGameplayRef> entities({List<int> components = const []}) =>
      _guard(() {
        final w = _GameplayWriter(5)..u32(components.length);
        for (final id in components) {
          w.u32(id);
        }
        final r = _send(w), result = <NativeGameplayRef>[];
        final n = r.u32();
        for (var i = 0; i < n; i++) {
          result.add(_ref(r)!);
        }
        return result;
      });
  String? name(NativeGameplayRef entity) => _guard(() {
    final r = _send(_GameplayWriter(6)..reference(entity));
    return r.u32() == 0 ? null : r.string();
  });
  void setName(NativeGameplayRef entity, String name) => _send(
    _GameplayWriter(7)
      ..reference(entity)
      ..string(name),
  );
  NativeGameplayRef? parent(NativeGameplayRef entity) =>
      _guard(() => _ref(_send(_GameplayWriter(8)..reference(entity))));
  void setParent(
    NativeGameplayRef entity,
    NativeGameplayRef? parent, {
    bool keepWorld = false,
  }) => _send(
    _GameplayWriter(9)
      ..reference(entity)
      ..reference(parent)
      ..u32(keepWorld ? 1 : 0),
  );
  List<NativeGameplayRef> children(NativeGameplayRef entity) => _guard(() {
    final r = _send(_GameplayWriter(10)..reference(entity));
    final n = r.u32();
    return List.generate(n, (_) => _ref(r)!);
  });
  (bool, bool) active(NativeGameplayRef entity) => _guard(() {
    final r = _send(_GameplayWriter(11)..reference(entity));
    return (r.u32() != 0, r.u32() != 0);
  });
  void setActive(NativeGameplayRef entity, bool active) => _send(
    _GameplayWriter(12)
      ..reference(entity)
      ..u32(active ? 1 : 0),
  );
  Float32List worldMatrix(NativeGameplayRef entity) => _guard(() {
    final r = _send(_GameplayWriter(13)..reference(entity));
    return Float32List.fromList(List.generate(16, (_) => r.f32()));
  });
  bool contains(NativeGameplayRef entity) =>
      _guard(() => _send(_GameplayWriter(14)..reference(entity)).u32() != 0);
  void remove<T extends WireRecord>(NativeGameplayRef entity) => _guard(() {
    final id = gen.componentIds[T];
    if (id == null) throw ArgumentError('unregistered component $T');
    _send(
      _GameplayWriter(15)
        ..reference(entity)
        ..u32(id),
    );
  });
  NativeGameplayRef? find(String name) =>
      _guard(() => _ref(_send(_GameplayWriter(16)..string(name))));
}

class _GameplayWriter {
  _GameplayWriter(int op) {
    u32(1); // protocol version
    u32(op);
  }
  final bytes = BytesBuilder(copy: false);
  void u32(int value) {
    final b = ByteData(4)..setUint32(0, value, Endian.little);
    bytes.add(b.buffer.asUint8List());
  }

  void u64(int value) {
    final b = ByteData(8)..setUint64(0, value, Endian.little);
    bytes.add(b.buffer.asUint8List());
  }

  void string(String value) {
    final b = utf8.encode(value);
    u32(b.length);
    bytes.add(b);
  }

  void scalar(NativeRegisteredValue v) {
    u32(v.kind);
    switch (v.kind) {
      case 0:
        final value = v.value as double;
        if (!value.isFinite) {
          throw ArgumentError('component float must be finite');
        }
        bytes.add(
          (ByteData(
            8,
          )..setFloat64(0, value, Endian.little)).buffer.asUint8List(),
        );
      case 1:
        u64(v.value as int);
      case 2:
        u32((v.value as bool) ? 1 : 0);
      case 3:
        string(v.value as String);
      case 5:
        final asset = v.value as AssetRef;
        string(asset.asset);
        string(asset.sub ?? '');
      case 4:
        reference(v.value as NativeGameplayRef?);
      default:
        throw ArgumentError('invalid component field kind');
    }
  }

  void scalars(List<NativeRegisteredValue> values) {
    u32(values.length);
    for (final v in values) {
      scalar(v);
    }
  }

  void reference(NativeGameplayRef? r) {
    if (r == null) {
      bytes.add(Uint8List(32));
      return;
    }
    if (r.isPending) {
      u32(2);
      u32(r._index);
      u64(r._world);
      u32(0);
      u32(0);
      u64(r._sequence);
    } else {
      u32(1);
      u32(0);
      u64(r.live!.world);
      u32(r.live!.slot);
      u32(r.live!.generation);
      u64(0);
    }
  }
}

class _GameplayReader {
  _GameplayReader(this.bytes) : data = ByteData.sublistView(bytes);
  final Uint8List bytes;
  final ByteData data;
  int offset = 0;
  int u32() {
    final v = data.getUint32(offset, Endian.little);
    offset += 4;
    return v;
  }

  int u64() {
    final v = data.getUint64(offset, Endian.little);
    offset += 8;
    return v;
  }

  double f32() {
    final v = data.getFloat32(offset, Endian.little);
    offset += 4;
    return v;
  }

  Uint8List take(int n) {
    final b = Uint8List.sublistView(bytes, offset, offset + n);
    offset += n;
    return b;
  }

  NativeRegisteredValue scalar() {
    final kind = u32();
    final Object? value;
    switch (kind) {
      case 0:
        value = data.getFloat64(offset, Endian.little);
        offset += 8;
      case 1:
        value = data.getInt64(offset, Endian.little);
        offset += 8;
      case 2:
        value = u32() != 0;
      case 3:
        value = string();
      case 5:
        final asset = string(), sub = string();
        value = AssetRef(asset, sub: sub.isEmpty ? null : sub);
      case 4:
        value = reference();
      default:
        throw FormatException('invalid component scalar kind');
    }
    return NativeRegisteredValue(kind, value);
  }

  List<NativeRegisteredValue> scalars() =>
      List.generate(u32(), (_) => scalar());

  String string() => utf8.decode(take(u32()));
  EntityRef entity() => EntityRef(u64(), u32(), u32());
  NativeGameplayRef? reference() {
    final kind = u32(), index = u32();
    final e = entity();
    final sequence = u64();
    return switch (kind) {
      0 => null,
      1 => NativeGameplayRef.live(e),
      2 => NativeGameplayRef._pending(e.world, sequence, index),
      _ => throw FormatException('invalid reference tag'),
    };
  }
}
