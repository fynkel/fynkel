// Error reporting across the ABI (ADR-0014): status codes + structured
// diagnostic records. Debug builds throw FynkelError; release builds log.

import '../generated/schema.g.dart' as gen;
import 'physics.dart';

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

/// One `inputActions` entry of the loaded project as the world bound it
/// (`LoadReport.inputActions`): the name, the kind and the bindings in
/// the project's grammar (`keyboard:WASD`, `touch:overlay/stick`, …).
class BoundAction {
  const BoundAction({required this.name, required this.type, required this.bindings});

  factory BoundAction.fromJson(Map<String, Object?> j) => BoundAction(
        name: j['name'] as String? ?? '',
        type: j['type'] as String? ?? 'button',
        bindings: [for (final b in (j['bindings'] as List?) ?? const []) b as String],
      );

  final String name;

  /// `button`, `axis1d` or `axis2d`.
  final String type;
  final List<String> bindings;

  /// The `touch:overlay/<control>` control names this action binds.
  Iterable<String> get overlayControls => bindings
      .where((b) => b.startsWith('touch:overlay/'))
      .map((b) => b.substring('touch:overlay/'.length));
}

/// What [World.instantiate] produced: the anchor is the returned object.
class InstantiateReport {
  const InstantiateReport({
    required this.ok,
    required this.diagnostics,
    this.prefabId,
    this.prefabPath,
    this.name = '',
    this.anchor = 0,
    this.entityCount = 0,
    this.rejectedComponents = 0,
    this.assetCount = 0,
    this.behaviours = const [],
    this.behavioursAttached = 0,
    this.residency = const ResidencyStats(),
  });

  factory InstantiateReport.fromJson(Map<String, Object?> j) {
    final inst = j['instance'] as Map<String, Object?>?;
    final residency = j['residency'] as Map<String, Object?>?;
    return InstantiateReport(
      ok: j['ok'] == true,
      diagnostics: [
        for (final d in (j['diagnostics'] as List?) ?? const []) LoadDiagnostic.fromJson(d as Map<String, Object?>),
      ],
      prefabId: inst?['prefab'] as String?,
      prefabPath: inst?['path'] as String?,
      name: inst?['name'] as String? ?? '',
      anchor: (inst?['anchor'] as num?)?.toInt() ?? 0,
      entityCount: (inst?['entities'] as num?)?.toInt() ?? 0,
      rejectedComponents: (inst?['rejectedComponents'] as num?)?.toInt() ?? 0,
      assetCount: (inst?['assets'] as num?)?.toInt() ?? 0,
      behaviours: [
        for (final b in (inst?['behaviours'] as List?) ?? const [])
          AuthoredBehaviour.fromJson((b as Map).cast<String, Object?>()),
      ],
      residency: residency == null ? const ResidencyStats() : ResidencyStats.fromJson(residency),
    );
  }

  final bool ok;
  final List<LoadDiagnostic> diagnostics;
  final String? prefabId;
  final String? prefabPath;

  /// The anchor's name.
  final String name;

  /// The anchor entity's handle.
  final int anchor;

  /// The prefab's entities spawned under the anchor (the anchor excluded).
  final int entityCount;
  final int rejectedComponents;
  final int assetCount;
  final List<AuthoredBehaviour> behaviours;
  final int behavioursAttached;
  final ResidencyStats residency;

  Iterable<LoadDiagnostic> get warnings => diagnostics.where((d) => d.severity == 'warning');

  InstantiateReport withBehaviours({required int attached, required List<LoadDiagnostic> extra}) => InstantiateReport(
        ok: ok,
        diagnostics: [...diagnostics, ...extra],
        prefabId: prefabId,
        prefabPath: prefabPath,
        name: name,
        anchor: anchor,
        entityCount: entityCount,
        rejectedComponents: rejectedComponents,
        assetCount: assetCount,
        behaviours: behaviours,
        behavioursAttached: attached,
        residency: residency,
      );
}

/// What [World.loadScene] / [World.loadProject] produced.
class LoadReport {
  const LoadReport({
    required this.ok,
    required this.diagnostics,
    this.sceneId,
    this.sceneName,
    this.scenePath,
    this.sceneKey,
    this.entityCount = 0,
    this.rejectedComponents = 0,
    this.assetCount = 0,
    this.behaviours = const [],
    this.behavioursAttached = 0,
    this.projectName,
    this.projectDir,
    this.cacheDir,
    this.cacheProfile,
    this.startupScene,
    this.residency = const ResidencyStats(),
    this.inputActions = const [],
    this.physics = const PhysicsSettings(),
  });

  factory LoadReport.fromJson(Map<String, Object?> j) {
    final scene = j['scene'] as Map<String, Object?>?;
    final cache = j['cache'] as Map<String, Object?>?;
    final residency = j['residency'] as Map<String, Object?>?;
    final input = j['input'] as Map<String, Object?>?;
    final physics = j['physics'] as Map<String, Object?>?;
    return LoadReport(
      ok: j['ok'] == true,
      diagnostics: [
        for (final d in (j['diagnostics'] as List?) ?? const []) LoadDiagnostic.fromJson(d as Map<String, Object?>),
      ],
      sceneId: scene?['id'] as String?,
      sceneName: scene?['name'] as String?,
      scenePath: scene?['path'] as String?,
      sceneKey: scene?['key'] as String?,
      entityCount: (scene?['entities'] as num?)?.toInt() ?? 0,
      rejectedComponents: (scene?['rejectedComponents'] as num?)?.toInt() ?? 0,
      assetCount: (scene?['assets'] as num?)?.toInt() ?? 0,
      behaviours: [
        for (final b in (scene?['behaviours'] as List?) ?? const [])
          AuthoredBehaviour.fromJson((b as Map).cast<String, Object?>()),
      ],
      projectName: cache?['project'] as String?,
      cacheDir: cache?['dir'] as String?,
      cacheProfile: cache?['profile'] as String?,
      startupScene: cache?['startupScene'] as String?,
      residency: residency == null ? const ResidencyStats() : ResidencyStats.fromJson(residency),
      inputActions: [
        for (final a in (input?['actions'] as List?) ?? const []) BoundAction.fromJson((a as Map).cast<String, Object?>()),
      ],
      physics: physics == null ? const PhysicsSettings() : PhysicsSettings.fromJson(physics),
    );
  }

  final bool ok;
  final List<LoadDiagnostic> diagnostics;
  final String? sceneId;
  final String? sceneName;

  /// The scene's authoring path as the cache index records it (relative to
  /// the asset root; for messages — the runtime never opens it).
  final String? scenePath;

  /// The cooked scene object's cache key (hex).
  final String? sceneKey;
  final int entityCount;
  final int rejectedComponents;

  /// Assets retained for the scene: the scene and its dependency closure.
  final int assetCount;

  /// The scene's authored behaviours (every non-native component).
  final List<AuthoredBehaviour> behaviours;

  /// How many of [behaviours] were instantiated and attached; the rest are
  /// `FYN-B` warnings in [diagnostics].
  final int behavioursAttached;

  /// The project name the cache was cooked for (its index).
  final String? projectName;

  /// The project directory passed to `World.loadProject` (null for
  /// `loadScene`).
  final String? projectDir;

  /// The cooked cache the scene came from.
  final String? cacheDir;

  /// The cook profile of that cache (`dev`, `ios`, …).
  final String? cacheProfile;

  /// The startup scene id the cache's index names.
  final String? startupScene;

  /// The world's asset store after the load.
  final ResidencyStats residency;

  /// The project's `inputActions` as the world bound them (a `loadProject`;
  /// empty for a `loadScene` of a cache without a project).
  final List<BoundAction> inputActions;

  /// The project's physics settings as applied (gravity, layer names, mesh
  /// colliders registered); defaults for a `loadScene`.
  final PhysicsSettings physics;

  Iterable<LoadDiagnostic> get errors => diagnostics.where((d) => d.isError);
  Iterable<LoadDiagnostic> get warnings => diagnostics.where((d) => !d.isError);

  LoadReport _copy({int? behavioursAttached, List<LoadDiagnostic>? diagnostics, String? projectDir}) => LoadReport(
        ok: ok,
        diagnostics: diagnostics ?? this.diagnostics,
        sceneId: sceneId,
        sceneName: sceneName,
        scenePath: scenePath,
        sceneKey: sceneKey,
        entityCount: entityCount,
        rejectedComponents: rejectedComponents,
        assetCount: assetCount,
        behaviours: behaviours,
        behavioursAttached: behavioursAttached ?? this.behavioursAttached,
        projectName: projectName,
        projectDir: projectDir ?? this.projectDir,
        cacheDir: cacheDir,
        cacheProfile: cacheProfile,
        startupScene: startupScene,
        residency: residency,
        inputActions: inputActions,
        physics: physics,
      );

  /// This report after the SDK attached [attached] behaviours and found
  /// [extra] problems doing so.
  LoadReport withBehaviours({required int attached, required List<LoadDiagnostic> extra}) => _copy(
        behavioursAttached: attached,
        diagnostics: extra.isEmpty ? diagnostics : [...diagnostics, ...extra],
      );

  /// This report with the project directory it was loaded for.
  LoadReport withProjectDir(String dir) => _copy(projectDir: dir);
}

/// What the world's asset store holds (`LoadReport.residency`).
class ResidencyStats {
  const ResidencyStats({this.retained = 0, this.meshes = 0, this.textures = 0, this.materials = 0, this.bytes = 0});

  factory ResidencyStats.fromJson(Map<String, Object?> j) => ResidencyStats(
        retained: (j['retained'] as num?)?.toInt() ?? 0,
        meshes: (j['meshes'] as num?)?.toInt() ?? 0,
        textures: (j['textures'] as num?)?.toInt() ?? 0,
        materials: (j['materials'] as num?)?.toInt() ?? 0,
        bytes: (j['bytes'] as num?)?.toInt() ?? 0,
      );

  /// Assets some loaded scene retains.
  final int retained;

  /// Payloads resident right now (loaded on first use, so 0 right after
  /// a load and before a frame is drawn).
  final int meshes;
  final int textures;
  final int materials;

  /// CPU bytes of the resident mesh and texture payloads.
  final int bytes;

  @override
  String toString() => 'residency(retained $retained, $meshes meshes, $textures textures, $materials materials, $bytes bytes)';
}

/// A scene or project failed to load; [report] says why.
/// [World.instantiate] failed; the world is unchanged.
class FynkelInstantiateError extends Error {
  FynkelInstantiateError(this.report);
  final InstantiateReport report;

  @override
  String toString() {
    final lines = report.diagnostics.map((d) => '  $d').join('\n');
    return 'FynkelInstantiateError:\n$lines';
  }
}

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
