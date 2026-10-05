// Standalone regression runner: dart run benchmarks/mobile/capture_gate_test.dart
import 'dart:async';
import 'dart:io';

import 'capture_gate.dart';

void check(bool condition, String message) {
  if (!condition) throw StateError(message);
}

Future<void> rejects<T>(Future<Object?> Function() operation) async {
  try {
    await operation();
  } catch (error) {
    if (error is T) return;
    rethrow;
  }
  throw StateError('Expected $T');
}

Future<void> main() async {
  final dir = await Directory.systemTemp.createTemp('fynkel-capture-gate-');
  const id = '0123456789abcdef0123456789abcdef';
  final signal = File('${dir.path}/capture-go-$id.json');
  Future<bool> wait({Duration timeout = const Duration(seconds: 5)}) =>
      waitForCaptureRelease(
        dir,
        id,
        isActive: () => true,
        timeout: timeout,
        pollInterval: const Duration(milliseconds: 1),
      );
  try {
    await rejects<ArgumentError>(
      () => waitForCaptureRelease(dir, '../invalid', isActive: () => true),
    );
    await signal.writeAsString('{"captureId":"$id"}');
    check(await wait(), 'matching signal must release');
    check(!await signal.exists(), 'signal must be consumed');
    await rejects<TimeoutException>(() => wait(timeout: Duration.zero));

    // A valid signal for another launch must not release this one.
    final stale = File(
      '${dir.path}/capture-go-ffffffffffffffffffffffffffffffff.json',
    );
    await stale.writeAsString(
      '{"captureId":"ffffffffffffffffffffffffffffffff"}',
    );
    await rejects<TimeoutException>(
      () => wait(timeout: const Duration(milliseconds: 10)),
    );
    await signal.writeAsString('{"captureId":"wrong"}');
    await rejects<StateError>(() => wait());
    await signal.writeAsString('null');
    await rejects<StateError>(() => wait());

    // Simulate a destination visible while devicectl is still transferring it.
    await signal.writeAsString('{"captureId":');
    final transfer = Future<void>.delayed(
      const Duration(milliseconds: 20),
      () async {
        await signal.writeAsString('{"captureId":"$id"}');
      },
    );
    check(await wait(), 'partial transfer must wait for the complete signal');
    await transfer;
    await signal.writeAsString('{');
    await rejects<TimeoutException>(
      () => wait(timeout: const Duration(milliseconds: 10)),
    );

    check(
      !await waitForCaptureRelease(dir, id, isActive: () => false),
      'cancelled capture must not release',
    );
    var active = true;
    final cancellation = Future<void>.delayed(
      const Duration(milliseconds: 10),
      () {
        active = false;
      },
    );
    check(
      !await waitForCaptureRelease(
        dir,
        id,
        isActive: () => active,
        pollInterval: const Duration(milliseconds: 1),
      ),
      'cancellation during wait',
    );
    await cancellation;
    print(
      'PASS: identity, one-shot release, stale/mismatched/malformed signals, partial transfer, timeout and cancellation',
    );
  } finally {
    await dir.delete(recursive: true);
  }
}
