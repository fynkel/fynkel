// Explicit experimental library; independent of the shipping native-assets ABI.
import 'dart:convert';
import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

final class NativeStorage {
  NativeStorage(DynamicLibrary library, this.count)
    : _create = library
          .lookupFunction<
            Pointer<Void> Function(Uint32),
            Pointer<Void> Function(int)
          >('foundation_create'),
      _snapshot = library
          .lookupFunction<
            Pointer<Void> Function(Pointer<Void>, Pointer<Uint64>),
            Pointer<Void> Function(Pointer<Void>, Pointer<Uint64>)
          >('foundation_snapshot'),
      _commit = library
          .lookupFunction<
            Int32 Function(Pointer<Void>, Uint64),
            int Function(Pointer<Void>, int)
          >('foundation_commit'),
      _dispose = library
          .lookupFunction<
            Void Function(Pointer<Void>),
            void Function(Pointer<Void>)
          >('foundation_dispose') {
    _pointer = _create(count);
    if (_pointer == nullptr) {
      calloc.free(_token);
      throw StateError('Native candidate creation failed');
    }
  }
  final int count;
  final Pointer<Void> Function(int) _create;
  final Pointer<Void> Function(Pointer<Void>, Pointer<Uint64>) _snapshot;
  final int Function(Pointer<Void>, int) _commit;
  final void Function(Pointer<Void>) _dispose;
  final Pointer<Uint64> _token = calloc<Uint64>();
  late final Pointer<Void> _pointer;
  bool _disposed = false;
  int _generation = 0, calls = 0;

  NativeView snapshot() {
    if (_disposed) throw StateError('Disposed candidate kernel');
    calls++;
    final pointer = _snapshot(_pointer, _token);
    if (pointer == nullptr) throw StateError('Native snapshot failed');
    return NativeView._(
      this,
      ++_generation,
      _token.value,
      pointer.cast<Double>().asTypedList(count * 4),
    );
  }

  int commit(NativeView view) {
    view._check();
    if (!identical(view._owner, this)) {
      throw StateError('Foreign candidate view');
    }
    calls++;
    final result = _commit(_pointer, view._token);
    _generation++; // A consumed or rejected snapshot is no longer editable.
    return result;
  }

  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _generation++;
    _dispose(_pointer);
    calloc.free(_token);
  }
}

final class NativeView {
  NativeView._(this._owner, this._generation, this._token, this._rows);
  final NativeStorage _owner;
  final int _generation, _token;
  final Float64List _rows;
  void _check() {
    if (_owner._disposed || _generation != _owner._generation) {
      throw StateError('Expired candidate snapshot');
    }
  }

  void setHp(int index, double value) {
    _check();
    if (index < 0 || index >= _owner.count) {
      throw RangeError.index(index, _rows);
    }
    _rows[index * 4 + 2] =
        value; // Opaque 128-bit entity in columns zero and one remains untouched.
  }

  void poison(double dt) {
    _check();
    for (var row = 0; row < _rows.length; row += 4) {
      final hp = _rows[row + 2] - _rows[row + 3] * dt;
      _rows[row + 2] = hp < 0 ? 0 : hp;
    }
  }

  double checksum() {
    _check();
    var total = 0.0;
    for (var row = 0; row < _rows.length; row += 4) {
      total += _rows[row + 2];
    }
    return total;
  }
}

Map<String, Object?> contracts(DynamicLibrary library) {
  final store = NativeStorage(library, 2);
  try {
    final invalid = store.snapshot();
    invalid.setHp(0, 1);
    invalid.setHp(1, double.nan);
    if (store.commit(invalid) != 1) {
      throw StateError('Invalid native batch accepted');
    }
    final restored = store.snapshot();
    if (restored.checksum() != 200) {
      throw StateError('Invalid native batch changed authority');
    }
    final next = store.snapshot();
    var expired = false;
    try {
      restored.checksum();
    } on StateError {
      expired = true;
    }
    if (!expired || store.commit(next) != 0) {
      throw StateError('Snapshot lifetime contract failed');
    }
    store.dispose();
    var disposed = false;
    try {
      next.checksum();
    } on StateError {
      disposed = true;
    }
    if (!disposed) throw StateError('Disposed snapshot remained valid');
    return {
      'invalidBatchRolledBack': true,
      'expiredSnapshotRejected': true,
      'disposedSnapshotRejected': true,
    };
  } finally {
    store.dispose();
  }
}

Map<String, Object?> benchmark(DynamicLibrary library, int count) {
  final store = NativeStorage(library, count);
  try {
    for (var round = 0; round < 20; round++) {
      final view = store.snapshot();
      view.poison(1 / 60);
      if (store.commit(view) != 0) throw StateError('Warm batch rejected');
    }
    final reset = store.snapshot();
    for (var row = 0; row < count; row++) {
      reset.setHp(row, 100);
    }
    if (store.commit(reset) != 0) throw StateError('Reset batch rejected');
    final calls = store.calls;
    final times = <int>[];
    const rounds = 120;
    for (var round = 0; round < rounds; round++) {
      final timer = Stopwatch()..start();
      final view = store.snapshot();
      view.poison(1 / 60);
      if (store.commit(view) != 0) throw StateError('Measured batch rejected');
      timer.stop();
      times.add(timer.elapsedMicroseconds);
    }
    final timedCalls = store.calls - calls;
    final checksum = store.snapshot().checksum();
    if ((checksum - count * 96).abs() > .0001 * count) {
      throw StateError('Wrong bridged benchmark output');
    }
    times.sort();
    return {
      'entities': count,
      'rounds': rounds,
      'p50Us': times[times.length ~/ 2],
      'p95Us': times[(times.length * .95).floor()],
      'checksum': checksum,
      'timedBridgeCalls': timedCalls,
      'snapshotBytesCopied': count * 32 * rounds,
      'timedPath': 'Native ECS snapshot copy; two FFI calls per round; Dart borrowed-view bulk edit; all-record validation and native commit',
      'limitations': 'Fixed Health layout and population, no structural changes, schemas, scheduling, production handle allocation or renderer integration; Dart JIT, not AOT',
    };
  } finally {
    store.dispose();
  }
}

void main(List<String> args) {
  final suffix = Platform.isMacOS
      ? '.dylib'
      : Platform.isWindows
      ? '.dll'
      : '.so';
  final path = args.isNotEmpty
      ? args.first
      : '../../target/release/examples/libfoundation_bridge$suffix';
  final library = DynamicLibrary.open(File(path).absolute.path);
  print(
    const JsonEncoder.withIndent('  ').convert({
      'experiment': 'F0 native authority with a batched Dart crossing; isolated candidate kernel',
      'contracts': contracts(library),
      'nativeOwnedDartLogic': [
        benchmark(library, 1000),
        benchmark(library, 10000),
      ],
    }),
  );
}
