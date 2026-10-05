// Diagnostic-only handshake. The workload must not start before this completes.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

Future<bool> waitForCaptureRelease(
  Directory directory,
  String captureId, {
  required bool Function() isActive,
  Duration timeout = const Duration(seconds: 45),
  Duration pollInterval = const Duration(milliseconds: 100),
}) async {
  if (!RegExp(r'^[0-9a-f]{32}$').hasMatch(captureId)) {
    throw ArgumentError.value(captureId, 'captureId');
  }
  final signal = File('${directory.path}/capture-go-$captureId.json');
  final waiting = Stopwatch()..start();
  while (isActive()) {
    if (waiting.elapsed >= timeout) {
      throw TimeoutException('Profiler start signal timed out', timeout);
    }
    if (await signal.exists()) {
      Object? value;
      var complete = false;
      try {
        value = jsonDecode(await signal.readAsString());
        complete = true;
      } on FormatException {
        // Device copy can expose the destination before all bytes arrive.
        // Keep waiting within the same deadline; malformed data never releases.
      }
      if (complete) {
        if (value is! Map<String, dynamic> || value['captureId'] != captureId) {
          throw StateError(
            'Profiler start signal belongs to a different capture',
          );
        }
        if (!isActive()) return false;
        await signal.delete();
        return isActive();
      }
    }
    await Future<void>.delayed(pollInterval);
  }
  return false;
}
