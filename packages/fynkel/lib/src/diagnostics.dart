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

  /// Runtime conditions that are reported, never thrown: a missed deadline
  /// is the clock dropping time after a stall (ADR-0008), not a program
  /// error, and a host must keep ticking through it.
  bool get isInformational => code == DiagnosticCode.missedDeadline;

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

/// One `fynkel_world_load_*` diagnostic (a document diagnostic: code
/// `FYN-D####`/`FYN-P####`/`FYN-A####`, the file and JSON pointer when
/// known) or an SDK one (`FYN-B####`: a behaviour the SDK could not attach).
class LoadDiagnostic {
  const LoadDiagnostic({
    required this.code,
    required this.severity,
    required this.message,
    this.document,
    this.pointer,
    this.line,
    this.column,
    this.suggestion,
  });

  factory LoadDiagnostic.fromJson(Map<String, Object?> j) => LoadDiagnostic(
        code: j['code'] as String? ?? '',
        severity: j['severity'] as String? ?? 'error',
        message: j['message'] as String? ?? '',
        document: j['document'] as String?,
        pointer: j['pointer'] as String?,
        line: (j['line'] as num?)?.toInt(),
        column: (j['column'] as num?)?.toInt(),
        suggestion: j['suggestion'] as String?,
      );

  final String code;
  final String severity;
  final String message;
  final String? document;
  final String? pointer;
  final int? line;
  final int? column;
  final String? suggestion;

  bool get isError => severity == 'error';

  @override
  String toString() {
    final where = [
      if (document != null) document,
      if (line != null) '$line${column != null ? ':$column' : ''}',
      if (pointer != null && pointer!.isNotEmpty) pointer,
    ].join(':');
    return '$severity $code${where.isEmpty ? '' : ' $where'}: $message${suggestion != null ? ' ($suggestion)' : ''}';
  }
}

/// One scene component of a non-native type in a load report: a Dart
/// behaviour the SDK instantiates from the registry (`FYN-B0001` when the
/// type is not registered, `FYN-B0002` when the data does not fit) and
/// attaches to the spawned [entity].
class AuthoredBehaviour {
  const AuthoredBehaviour({
    required this.entity,
    required this.authoringId,
    required this.component,
    required this.type,
    required this.version,
    required this.known,
    required this.data,
    this.pointer,
  });

  factory AuthoredBehaviour.fromJson(Map<String, Object?> j) => AuthoredBehaviour(
        entity: (j['entity'] as num).toInt(),
        authoringId: j['authoringId'] as String? ?? '',
        component: j['component'] as String? ?? '',
        type: j['type'] as String? ?? '',
        version: (j['version'] as num?)?.toInt() ?? 1,
        known: j['known'] == true,
        data: (j['data'] as Map?)?.cast<String, Object?>() ?? const {},
        pointer: j['pointer'] as String?,
      );

  /// The spawned entity's handle (what [World.attach] takes).
  final int entity;

  /// The entity's and the component's authoring ids.
  final String authoringId;
  final String component;

  /// The behaviour type id (`game.Spinner`) and authored version.
  final String type;
  final int version;

  /// Whether the document layer knew the type (a project schema or registry
  /// entry); false means it was reported as `FYN-D0205` there.
  final bool known;

  /// The authored `data` object: the fields to set.
  final Map<String, Object?> data;
  final String? pointer;
}

/// What [World.loadScene] / [World.loadProject] produced.
class LoadReport {
  const LoadReport({
    required this.ok,
    required this.diagnostics,
    this.sceneId,
    this.sceneName,
    this.scenePath,
    this.entityCount = 0,
    this.rejectedComponents = 0,
    this.behaviours = const [],
    this.behavioursAttached = 0,
    this.projectName,
    this.projectDir,
    this.assetsDir,
    this.cacheDir,
  });

  factory LoadReport.fromJson(Map<String, Object?> j) {
    final scene = j['scene'] as Map<String, Object?>?;
    final project = j['project'] as Map<String, Object?>?;
    return LoadReport(
      ok: j['ok'] == true,
      diagnostics: [
        for (final d in (j['diagnostics'] as List?) ?? const []) LoadDiagnostic.fromJson(d as Map<String, Object?>),
      ],
      sceneId: scene?['id'] as String?,
      sceneName: scene?['name'] as String?,
      scenePath: scene?['path'] as String?,
      entityCount: (scene?['entities'] as num?)?.toInt() ?? 0,
      rejectedComponents: (scene?['rejectedComponents'] as num?)?.toInt() ?? 0,
      behaviours: [
        for (final b in (scene?['behaviours'] as List?) ?? const [])
          AuthoredBehaviour.fromJson((b as Map).cast<String, Object?>()),
      ],
      projectName: project?['name'] as String?,
      projectDir: project?['dir'] as String?,
      assetsDir: project?['assets'] as String?,
      cacheDir: project?['cache'] as String?,
    );
  }

  final bool ok;
  final List<LoadDiagnostic> diagnostics;
  final String? sceneId;
  final String? sceneName;
  final String? scenePath;
  final int entityCount;
  final int rejectedComponents;

  /// The scene's authored behaviours (every non-native component).
  final List<AuthoredBehaviour> behaviours;

  /// How many of [behaviours] were instantiated and attached; the rest are
  /// `FYN-B` warnings in [diagnostics].
  final int behavioursAttached;
  final String? projectName;
  final String? projectDir;
  final String? assetsDir;
  /// Where `fynkel cook` writes for this project (`<dir>/.fynkel/cache`).
  final String? cacheDir;

  Iterable<LoadDiagnostic> get errors => diagnostics.where((d) => d.isError);
  Iterable<LoadDiagnostic> get warnings => diagnostics.where((d) => !d.isError);

  /// This report after the SDK attached [attached] behaviours and found
  /// [extra] problems doing so.
  LoadReport withBehaviours({required int attached, required List<LoadDiagnostic> extra}) => LoadReport(
        ok: ok,
        diagnostics: extra.isEmpty ? diagnostics : [...diagnostics, ...extra],
        sceneId: sceneId,
        sceneName: sceneName,
        scenePath: scenePath,
        entityCount: entityCount,
        rejectedComponents: rejectedComponents,
        behaviours: behaviours,
        behavioursAttached: attached,
        projectName: projectName,
        projectDir: projectDir,
        assetsDir: assetsDir,
        cacheDir: cacheDir,
      );
}

/// A scene or project failed to load; [report] says why.
class FynkelLoadError extends Error {
  FynkelLoadError(this.report, {this.context = ''});
  final LoadReport report;
  final String context;

  @override
  String toString() {
    final lines = report.diagnostics.map((d) => '  $d').join('\n');
    return 'FynkelLoadError${context.isEmpty ? '' : ' ($context)'}:\n$lines';
  }
}
