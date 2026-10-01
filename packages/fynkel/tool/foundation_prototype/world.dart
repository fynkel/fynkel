// F0 experiment only. No exports from the shipping gameplay API.
import 'package:fynkel/fynkel.dart' as engine;

enum FieldKind { number, boolean }

final class FieldSchema {
  const FieldSchema(this.name, this.kind);
  final String name;
  final FieldKind kind;
}

/// The experiment uses immutable records; arbitrary mutable objects are not supported.
final class ComponentType<T extends Object> {
  ComponentType({
    required this.id,
    required this.version,
    required List<FieldSchema> fields,
    required this.create,
    required this.encode,
    required this.decode,
    required this.validate,
  }) : fields = List.unmodifiable(fields);
  final String id;
  final int version;
  final List<FieldSchema> fields;
  final T Function() create;
  final Map<String, Object?> Function(T) encode;
  final T Function(Map<String, Object?>) decode;
  final void Function(T) validate;
  Type get dartType => T;

  T readDocument(Map<String, Object?> document) {
    if (document['type'] != id || document['version'] != version) {
      throw FormatException(
        'Unsupported component identity/version: $document',
      );
    }
    final payload = document['fields'];
    if (payload is! Map<String, Object?>) {
      throw const FormatException('Expected component fields');
    }
    final names = fields.map((field) => field.name).toSet();
    if (payload.keys.any((key) => !names.contains(key)) ||
        names.any((key) => !payload.containsKey(key))) {
      throw const FormatException('Unknown or missing component fields');
    }
    for (final field in fields) {
      final value = payload[field.name];
      final valid = switch (field.kind) {
        FieldKind.number => value is num && value.isFinite,
        FieldKind.boolean => value is bool,
      };
      if (!valid) throw FormatException('Invalid ${field.name}');
    }
    final value = decode(payload);
    validate(value);
    return value;
  }

  Map<String, Object?> document(T value) => {
    'type': id,
    'version': version,
    'fields': encode(value),
  };
}

/// Native GameObject is the entity; this reference adds owner/scene lifetime checks.
/// It does not allocate a second entity ID or fix the native handle ABI.
final class EntityRef {
  EntityRef._(this._owner, this._epoch, this._object);
  final PrototypeWorld _owner;
  final int _epoch;
  final engine.GameObject _object;
  engine.EntityRef get nativeHandle {
    _owner._requireEntity(this);
    return _object.entity;
  }
}

final class _Store<T extends Object> {
  final entities = <EntityRef>[];
  final values = <T>[];
  final slots = <EntityRef, int>{};
  T? get(EntityRef entity) {
    final slot = slots[entity];
    return slot == null ? null : values[slot];
  }

  void put(EntityRef entity, T? value) {
    final slot = slots[entity];
    if (value == null) {
      if (slot == null) return;
      final lastEntity = entities.removeLast();
      final lastValue = values.removeLast();
      slots.remove(entity);
      if (slot < entities.length) {
        entities[slot] = lastEntity;
        values[slot] = lastValue;
        slots[lastEntity] = slot;
      }
    } else if (slot == null) {
      slots[entity] = entities.length;
      entities.add(entity);
      values.add(value);
    } else {
      values[slot] = value;
    }
  }
}

final class SystemSpec {
  SystemSpec({
    required this.id,
    required this.run,
    List<ComponentType<Object>> reads = const [],
    List<ComponentType<Object>> writes = const [],
    List<String> after = const [],
  }) : reads = List.unmodifiable(reads),
       writes = List.unmodifiable(writes),
       after = List.unmodifiable(after);
  final String id;
  final void Function(GameplayAccess, double dt) run;
  final List<ComponentType<Object>> reads;
  final List<ComponentType<Object>> writes;
  final List<String> after;
}

/// Scoped access for one atomic custom-data callback. Retaining it is an error.
final class GameplayAccess {
  GameplayAccess._(this._world, this._reads, this._writes);
  final PrototypeWorld _world;
  final Set<ComponentType<Object>> _reads, _writes;
  final _pending = <ComponentType<Object>, Map<EntityRef, Object?>>{};
  bool _valid = true;

  void _check(ComponentType<Object> type, {bool write = false}) {
    if (!_valid) {
      throw StateError('Gameplay access expired at callback boundary');
    }
    _world._requireType(type);
    if (!(write
        ? _writes.contains(type)
        : _reads.contains(type) || _writes.contains(type))) {
      throw StateError('Undeclared ${write ? "write" : "read"}: ${type.id}');
    }
  }

  T? get<T extends Object>(EntityRef entity, ComponentType<T> type) {
    _check(type);
    _world._requireEntity(entity);
    final changes = _pending[type];
    if (changes != null && changes.containsKey(entity)) {
      return changes[entity] as T?;
    }
    return _world._store(type).get(entity);
  }

  void set<T extends Object>(EntityRef entity, ComponentType<T> type, T value) {
    _check(type, write: true);
    _world._requireEntity(entity);
    type.validate(value);
    (_pending[type] ??= {})[entity] = value;
    _world.recordWrites++;
  }

  void remove<T extends Object>(EntityRef entity, ComponentType<T> type) {
    _check(type, write: true);
    _world._requireEntity(entity);
    (_pending[type] ??= {})[entity] = null;
  }

  ComponentObject object(EntityRef entity) {
    _world._requireEntity(entity);
    return ComponentObject._(this, entity);
  }

  /// Membership at query opening; later additions appear in the next query.
  /// Record reads include this callback's pending writes. No per-entity FFI.
  void forEach<T extends Object>(
    ComponentType<T> type,
    void Function(EntityRef, T) visit,
  ) {
    _check(type);
    final store = _world._store(type);
    final pending = _pending[type];
    final additions = pending == null
        ? const <EntityRef>[]
        : pending.keys
              .where((entity) => !store.slots.containsKey(entity))
              .toList();
    // Store structure cannot change until this callback commits. This avoids
    // copying the committed population or allocating one query row per entity.
    for (final entity in store.entities) {
      final value = get(entity, type);
      if (value != null) visit(entity, value);
    }
    for (final entity in additions) {
      final value = get(entity, type);
      if (value != null) visit(entity, value);
    }
  }
}

final class ComponentObject {
  ComponentObject._(this._access, this.entity);
  final GameplayAccess _access;
  final EntityRef entity;
  T? get<T extends Object>(ComponentType<T> type) => _access.get(entity, type);
  void set<T extends Object>(ComponentType<T> type, T value) =>
      _access.set(entity, type, value);
}

final class _Driver extends engine.Behaviour {
  _Driver(this.owner);
  final PrototypeWorld owner;
  @override
  void fixedUpdate(engine.FixedUpdateContext context) {
    if (owner._ready) owner._runSystems(context.dt);
  }
}

/// Custom data is Dart-owned; built-in state stays in the real native world.
/// This experiment does not claim atomic edits spanning the two domains.
final class PrototypeWorld {
  PrototypeWorld({engine.NativeLibrary? library})
    : _native = engine.World.create(library: library) {
    _native.spawn(name: 'prototype-scheduler').addBehaviour(_Driver(this));
  }
  final engine.World _native;
  final _stores = <ComponentType<Object>, Object>{};
  final _ids = <String, ComponentType<Object>>{};
  final _types = <Type>{};
  final _entities = <EntityRef>[];
  final _systems = <SystemSpec>[];
  List<SystemSpec>? _schedule;
  GameplayAccess? _active;
  int _epoch = 0;
  bool _disposed = false, _ready = false;
  int recordWrites = 0, commits = 0;

  void _idle() {
    if (_disposed) throw StateError('Disposed prototype world');
    if (_active != null) {
      throw StateError('Operation requires a callback boundary');
    }
  }

  void register<T extends Object>(ComponentType<T> type) {
    _idle();
    if (_ready) throw StateError('Register types before starting the slice');
    if (!RegExp(r'^[a-z][a-z0-9_]*(\.[a-zA-Z][a-zA-Z0-9_]*)+$')
            .hasMatch(type.id) ||
        type.version < 1) {
      throw ArgumentError('Expected namespaced ID and positive version');
    }
    if (_ids.containsKey(type.id) || _types.contains(type.dartType)) {
      throw StateError('Duplicate component registration');
    }
    if (type.fields.map((field) => field.name).toSet().length !=
        type.fields.length) {
      throw ArgumentError('Duplicate field names');
    }
    // Registration verifies defaults through the same inspector/loader codec.
    type.readDocument(type.document(type.create()));
    _ids[type.id] = type;
    _types.add(type.dartType);
    _stores[type] = _Store<T>();
  }

  void _requireType(ComponentType<Object> type) {
    if (!identical(_ids[type.id], type)) {
      throw StateError('Unregistered component descriptor');
    }
  }

  _Store<T> _store<T extends Object>(ComponentType<T> type) =>
      _stores[type] as _Store<T>;

  void _requireEntity(EntityRef entity) {
    if (_disposed ||
        !identical(entity._owner, this) ||
        entity._epoch != _epoch ||
        !entity._object.isAlive) {
      throw StateError('Foreign, retired or destroyed entity reference');
    }
    if (entity._object.isPending) {
      throw StateError('Entity awaits the setup commit');
    }
  }

  EntityRef spawn(String name) {
    _idle();
    if (_ready) {
      throw StateError(
        'Runtime spawning is not implemented by this experiment',
      );
    }
    final entity = EntityRef._(this, _epoch, _native.spawn(name: name));
    _entities.add(entity);
    return entity;
  }

  void initialize() {
    _idle();
    if (_ready) throw StateError('Slice already initialized');
    _native.frame(_native.fixedDt);
    _ready = true;
  }

  void addSystem(SystemSpec system) {
    _idle();
    if (_ready) throw StateError('Register systems before starting the slice');
    if (_systems.any((entry) => entry.id == system.id)) {
      throw StateError('Duplicate system ID');
    }
    for (final type in [...system.reads, ...system.writes]) {
      _requireType(type);
    }
    _systems.add(system);
    _schedule = null;
  }

  List<SystemSpec> _ordered() {
    if (_schedule != null) return _schedule!;
    final byId = {for (final system in _systems) system.id: system};
    for (final system in _systems) {
      if (system.after.any((id) => !byId.containsKey(id))) {
        throw StateError('Missing dependency for ${system.id}');
      }
    }
    final remaining = _systems.toList()..sort((a, b) => a.id.compareTo(b.id));
    final done = <String>{}, ordered = <SystemSpec>[];
    while (remaining.isNotEmpty) {
      final index = remaining.indexWhere(
        (system) => system.after.every(done.contains),
      );
      if (index < 0) throw StateError('System dependency cycle');
      final system = remaining.removeAt(index);
      ordered.add(system);
      done.add(system.id);
    }
    return _schedule = ordered;
  }

  List<String> get systemOrder =>
      _ordered().map((system) => system.id).toList();

  void transaction({
    List<ComponentType<Object>> reads = const [],
    List<ComponentType<Object>> writes = const [],
    required void Function(GameplayAccess) run,
  }) {
    _idle();
    if (!_ready) {
      throw StateError('Initialize real entity handles before accessing data');
    }
    final access = GameplayAccess._(this, reads.toSet(), writes.toSet());
    for (final type in [...reads, ...writes]) {
      _requireType(type);
    }
    _active = access;
    try {
      run(access);
      // No user callbacks or codecs at commit: all writes were validated.
      for (final entry in access._pending.entries) {
        final store = _store(entry.key);
        for (final change in entry.value.entries) {
          store.put(change.key, change.value);
        }
      }
      commits++;
    } finally {
      access._valid = false;
      _active = null;
    }
  }

  void _runSystems(double dt) {
    for (final system in _ordered()) {
      transaction(
        reads: system.reads,
        writes: system.writes,
        run: (access) => system.run(access, dt),
      );
    }
  }

  void frame(double dt) {
    _idle();
    if (!_ready) throw StateError('Initialize first');
    _ordered(); // Fail before advancing native simulation on invalid schedules.
    _native.frame(dt);
  }

  /// Inspector edits, host edits and behaviours use the same transaction route.
  void edit<T extends Object>(
    EntityRef entity,
    ComponentType<T> type,
    Map<String, Object?> fields,
  ) {
    transaction(
      writes: [type],
      run: (access) {
        final previous = access.get(entity, type) ?? type.create();
        final value = type.readDocument({
          'type': type.id,
          'version': type.version,
          'fields': {...type.encode(previous), ...fields},
        });
        access.set(entity, type, value);
      },
    );
  }

  Map<String, Object?> save<T extends Object>(
    EntityRef entity,
    ComponentType<T> type,
  ) {
    late Map<String, Object?> document;
    transaction(
      reads: [type],
      run: (access) {
        final value = access.get(entity, type);
        if (value == null) throw StateError('Missing component');
        document = type.document(value);
      },
    );
    return document;
  }

  void load<T extends Object>(
    EntityRef entity,
    ComponentType<T> type,
    Map<String, Object?> document,
  ) {
    final value = type.readDocument(document);
    transaction(
      writes: [type],
      run: (access) => access.set(entity, type, value),
    );
  }

  /// Explicit setup/scene boundary; runtime structural commands remain unproven.
  void replaceScene() {
    _idle();
    _ready = false;
    for (final entity in _entities) {
      entity._object.destroy();
    }
    _native.frame(_native.fixedDt);
    _entities.clear();
    for (final store in _stores.values) {
      final typed = store as _Store<Object>;
      typed.entities.clear();
      typed.values.clear();
      typed.slots.clear();
    }
    _epoch++;
  }

  void dispose() {
    if (_disposed) return;
    _idle();
    _native.dispose();
    _disposed = true;
  }
}
