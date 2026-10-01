// F0 shared native state candidate. JSON transport measures semantics, not final ABI performance.
import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

import 'slice.dart';

const mixedFields = [
  'Name',
  'Transform',
  'Hierarchy',
  'Activity',
  'Health',
  'DoorState',
  'Structure',
];

final class MixedRef {
  MixedRef._(this._owner, this._value);
  final MixedWorld _owner;
  Map<String, Object?> _value;
  bool _retired = false;
}

final class MixedWorld {
  MixedWorld({bool adapter = false, String? libraryPath}) {
    final extension = Platform.isMacOS
        ? 'dylib'
        : Platform.isWindows
        ? 'dll'
        : 'so';
    final prefix = Platform.isWindows ? '' : 'lib';
    final library = DynamicLibrary.open(
      File(
        libraryPath ??
            '../../target/release/examples/${prefix}foundation_state.$extension',
      ).absolute.path,
    );
    final create = library
        .lookupFunction<
          Pointer<Void> Function(Uint32),
          Pointer<Void> Function(int)
        >('foundation_state_create');
    _call = library
        .lookupFunction<
          Pointer<Uint8> Function(
            Pointer<Void>,
            Pointer<Uint8>,
            UintPtr,
            Pointer<UintPtr>,
          ),
          Pointer<Uint8> Function(
            Pointer<Void>,
            Pointer<Uint8>,
            int,
            Pointer<UintPtr>,
          )
        >('foundation_state_call');
    _dispose = library
        .lookupFunction<
          Void Function(Pointer<Void>),
          void Function(Pointer<Void>)
        >('foundation_state_dispose');
    _pointer = create(adapter ? 1 : 0);
    if (_pointer == nullptr) {
      throw StateError('Candidate namespace exhausted or creation failed');
    }
  }
  late final Pointer<Void> _pointer;
  late final Pointer<Uint8> Function(
    Pointer<Void>,
    Pointer<Uint8>,
    int,
    Pointer<UintPtr>,
  )
  _call;
  late final void Function(Pointer<Void>) _dispose;
  bool _disposed = false;
  MixedAccess? _active;
  final _refs = <String, MixedRef>{};
  int calls = 0, requestBytes = 0, responseBytes = 0;

  Object? _request(Map<String, Object?> request) {
    try {
      return _rawRequest(request);
    } catch (_) {
      _active?._failed = true;
      rethrow;
    }
  }

  Object? _rawRequest(Map<String, Object?> request) {
    if (_disposed) throw StateError('Disposed candidate world');
    final bytes = utf8.encode(jsonEncode(request));
    final input = calloc<Uint8>(bytes.length);
    final length = calloc<UintPtr>();
    try {
      input.asTypedList(bytes.length).setAll(0, bytes);
      calls++;
      requestBytes += bytes.length;
      final output = _call(_pointer, input, bytes.length, length);
      if (output == nullptr) throw StateError('Native candidate call failed');
      responseBytes += length.value;
      // Decode before another call can invalidate the borrowed output buffer.
      final result = jsonDecode(
        utf8.decode(output.asTypedList(length.value)),
      ) as Map<String, Object?>;
      if (result.containsKey('error')) {
        throw StateError(result['error'].toString());
      }
      return result['ok'];
    } finally {
      calloc.free(input);
      calloc.free(length);
    }
  }

  MixedRef _reference(Map<String, Object?> value) {
    if (value.containsKey('temp')) return _active!._born[value['temp'] as int]!;
    final id = value['entity'] as String;
    return _refs.putIfAbsent(id, () => MixedRef._(this, value));
  }

  void transaction({
    List<String> reads = mixedFields,
    List<String> writes = const [],
    required void Function(MixedAccess) run,
  }) {
    if (_active != null) throw StateError('Nested transaction');
    _request({'op': 'begin', 'reads': reads, 'writes': writes});
    final access = MixedAccess._(this);
    _active = access;
    var committed = false;
    try {
      run(access);
      if (access._failed) {
        throw StateError('Validation failed; callback must roll back');
      }
      final result = _request({'op': 'commit'}) as Map<String, Object?>;
      final created = result['created'] as Map<String, Object?>;
      for (final entry in access._born.entries) {
        final id = created[entry.key.toString()] as String?;
        if (id == null) {
          entry.value._retired = true;
          continue;
        }
        entry.value._value = {'entity': id};
        _refs[id] = entry.value;
      }
      committed = true;
    } finally {
      try {
        if (!committed) {
          for (final ref in access._born.values) {
            ref._retired = true;
          }
          _request({'op': 'abort'});
        }
      } finally {
        access._valid = false;
        _active = null;
      }
    }
  }

  Map<String, Object?> publish() {
    if (_active != null) {
      throw StateError('Publish requires a callback boundary');
    }
    return _request({'op': 'publish'}) as Map<String, Object?>;
  }

  void dispose() {
    if (_disposed) return;
    if (_active != null) {
      throw StateError('Dispose requires a callback boundary');
    }
    _disposed = true;
    _dispose(_pointer);
  }
}

final class MixedAccess {
  MixedAccess._(this._world);
  final MixedWorld _world;
  final _born = <int, MixedRef>{};
  bool _valid = true;
  bool _failed = false;
  void _check([MixedRef? ref]) {
    if (!_valid) throw StateError('Expired callback access');
    if (ref != null && (!identical(ref._owner, _world) || ref._retired)) {
      _failed = true;
      throw StateError('Foreign or retired reference');
    }
  }

  Map<String, Object?> _key(MixedRef ref) {
    _check(ref);
    return ref._value;
  }

  MixedObject object(MixedRef ref) {
    _check(ref);
    return MixedObject._(this, ref);
  }

  MixedRef spawn(String name, {MixedRef? parent}) {
    _check();
    final value = _world._request({
      'op': 'spawn',
      'name': name,
      'parent': parent == null ? null : _key(parent),
    }) as Map<String, Object?>;
    final ref = MixedRef._(_world, value);
    _born[value['temp'] as int] = ref;
    return ref;
  }

  Object? get(MixedRef ref, String field) =>
      _world._request({'op': 'get', 'ref': _key(ref), 'field': field});
  void set(MixedRef ref, String field, Object? value) {
    _world._request({
      'op': 'set',
      'ref': _key(ref),
      'field': field,
      'value': value,
    });
  }

  void destroy(MixedRef ref) {
    _world._request({'op': 'destroy', 'ref': _key(ref)});
  }

  void reparent(MixedRef ref, MixedRef? parent) =>
      set(ref, 'Hierarchy', parent == null ? null : _key(parent));

  MixedRef? find(String name) {
    _check();
    final result = _world._request({'op': 'find', 'name': name});
    return result == null
        ? null
        : _world._reference(result as Map<String, Object?>);
  }

  /// Candidate set is captured at query opening. Values read through current transaction.
  void forEach(String field, void Function(MixedObject) run) {
    _check();
    final result =
        _world._request({'op': 'query', 'field': field}) as List<Object?>;
    final refs = result
        .map(
          (row) => _world._reference(
            (row as Map<String, Object?>)['ref'] as Map<String, Object?>,
          ),
        )
        .toList();
    for (final ref in refs) {
      if (_world._request({'op': 'exists', 'ref': _key(ref)}) == true) {
        run(object(ref));
      }
    }
  }
}

final class MixedObject {
  MixedObject._(this._access, this.ref);
  final MixedAccess _access;
  final MixedRef ref;
  String get name => _access.get(ref, 'Name') as String;
  set name(String value) => _access.set(ref, 'Name', value);
  Health? get health {
    final fields = _access.get(ref, 'Health') as Map<String, Object?>?;
    return fields == null ? null : healthType.decode(fields);
  }

  set health(Health? value) => _access.set(
    ref,
    'Health',
    value == null ? null : healthType.encode(value),
  );
  DoorState? get door {
    final fields = _access.get(ref, 'DoorState') as Map<String, Object?>?;
    return fields == null ? null : doorType.decode(fields);
  }

  set door(DoorState? value) => _access.set(
    ref,
    'DoorState',
    value == null ? null : doorType.encode(value),
  );
  Map<String, Object?> get transform =>
      _access.get(ref, 'Transform') as Map<String, Object?>;
  set localTransform(Map<String, Object?> value) =>
      _access.set(ref, 'Transform', value);
  Map<String, Object?> get activity =>
      _access.get(ref, 'Activity') as Map<String, Object?>;
  set activeSelf(bool value) => _access.set(ref, 'Activity', value);

  /// Same validation route for metadata-driven inspector edits and ordinary writes.
  void editHealth(Map<String, Object?> fields) {
    final previous = health ?? const Health();
    try {
      health = healthType.readDocument({
        'type': healthType.id,
        'version': healthType.version,
        'fields': {...healthType.encode(previous), ...fields},
      });
    } catch (_) {
      _access._failed = true;
      rethrow;
    }
  }
}
