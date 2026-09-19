// Error reporting across the ABI (ADR-0014): status codes + structured
// diagnostic records. Debug builds throw FynkelError; release builds log.

import '../generated/schema.g.dart' as gen;

/// True in debug mode (asserts enabled), false in profile/release.
final bool isDebugMode = () {
  var debug = false;
  assert(debug = true);
  return debug;
}();

/// Stable diagnostic codes (fynkel_contracts::DiagnosticCode).
enum DiagnosticCode {
  unknown(0),
  unknownCommand(1),
  staleEntity(2),
  invalidTempId(3),
  invalidPayload(4),
  unknownComponent(5),
  missingComponent(6),
  hierarchyCycle(7),
  wrongWorld(8),
  physicsOwned(9),
  missedDeadline(10),
  duplicateComponent(11),
  wrongLane(12);

  const DiagnosticCode(this.value);
  final int value;

  static DiagnosticCode fromValue(int v) =>
      values.firstWhere((c) => c.value == v, orElse: () => DiagnosticCode.unknown);
}

class FynkelDiagnostic {
  const FynkelDiagnostic({
    required this.commandIndex,
    required this.code,
    required this.rawCode,
    required this.entity,
    required this.component,
    required this.lane,
    required this.message,
  });

  factory FynkelDiagnostic.fromRecord(gen.DiagnosticRecord r) => FynkelDiagnostic(
        commandIndex: r.commandIndex,
        code: DiagnosticCode.fromValue(r.code),
        rawCode: r.code,
        entity: r.entity,
        component: r.component,
        lane: r.lane,
        message: r.message,
      );

  /// Index in the submitted batch; `0xffffffff` when not tied to a command.
  final int commandIndex;
  final DiagnosticCode code;
  final int rawCode;
  final int entity;
  final int component;
  final int lane;
  final String message;

  bool get isCommandRejection => commandIndex != 0xffffffff;

  @override
  String toString() =>
      'FynkelDiagnostic(#${isCommandRejection ? commandIndex : '-'} ${code.name} entity=0x${entity.toRadixString(16)} lane=$lane "$message")';
}

/// Thrown in debug mode after a step/submit that produced diagnostics.
class FynkelError extends Error {
  FynkelError(this.diagnostics, {this.context = ''});
  final List<FynkelDiagnostic> diagnostics;
  final String context;

  @override
  String toString() {
    final b = StringBuffer('FynkelError');
    if (context.isNotEmpty) b.write(' ($context)');
    b.write(': ${diagnostics.length} diagnostic(s)');
    for (final d in diagnostics) {
      b.write('\n  $d');
    }
    return b.toString();
  }
}

/// Fatal bridge failure (protocol error, poisoned world, panic).
class FynkelBridgeError extends Error {
  FynkelBridgeError(this.message);
  final String message;
  @override
  String toString() => 'FynkelBridgeError: $message';
}
