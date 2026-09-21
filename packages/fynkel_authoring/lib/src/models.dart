// Typed views over the JSON the authoring ABI writes. Every model keeps the
// raw map in `json` so a panel can reach a field these classes do not name.

import 'diagnostic.dart';

Map<String, Object?> _map(Object? v) => v is Map<String, Object?> ? v : const {};
List<String> _strings(Object? v) => v is List ? [for (final s in v) if (s is String) s] : const [];

/// The open project as `fynkel_authoring::OpenProject::json()` describes it.
final class ProjectInfo {
  ProjectInfo.fromJson(this.json)
      : name = json['name'] as String? ?? '',
        dir = json['dir'] as String? ?? '',
        assetsDir = json['assetsDir'] as String? ?? '',
        readOnly = json['readOnly'] as String?,
        hasMetadata = json['hasMetadata'] == true,
        assetCount = (json['assetCount'] as num?)?.toInt() ?? 0,
        referenceCount = (json['referenceCount'] as num?)?.toInt() ?? 0,
        componentTypes = _strings(json['componentTypes']),
        behaviourTypes = _strings(json['behaviourTypes']),
        diagnostics = Diagnostic.listOf(json['diagnostics']);

  final Map<String, Object?> json;
  final String name;

  /// Absolute project directory.
  final String dir;
  final String assetsDir;

  /// Why the project is read-only (a lock written by a newer Fynkel), or
  /// null when it is editable.
  final String? readOnly;

  /// `lib/generated/behaviours.g.json` was found.
  final bool hasMetadata;
  final int assetCount;
  final int referenceCount;
  final List<String> componentTypes;
  final List<String> behaviourTypes;

  /// The project's own diagnostics at open (file, lock, scan).
  final List<Diagnostic> diagnostics;

  bool get isReadOnly => readOnly != null;
}

/// One open document's state — what the tab strip and the undo menu show.
final class DocumentState {
  DocumentState.fromJson(this.json)
      : handle = (json['handle'] as num?)?.toInt() ?? 0,
        path = json['path'] as String? ?? '',
        relativePath = json['relativePath'] as String?,
        kind = json['kind'] as String? ?? '',
        id = json['id'] as String? ?? '',
        name = json['name'] as String? ?? '',
        formatVersion = (json['formatVersion'] as num?)?.toInt() ?? 0,
        revision = json['revision'] as String? ?? '',
        dirty = json['dirty'] == true,
        canUndo = json['canUndo'] == true,
        canRedo = json['canRedo'] == true,
        history = (json['history'] as num?)?.toInt() ?? 0,
        readOnly = json['readOnly'] as String?;

  final Map<String, Object?> json;
  final int handle;

  /// Absolute path the document was read from (or last saved to).
  final String path;

  /// Path relative to the project's assets directory, when under it.
  final String? relativePath;

  /// `scene`, `prefab` or `material`.
  final String kind;
  final String id;
  final String name;
  final int formatVersion;

  /// Content hash of the document now; `apply` takes it as `baseRevision`.
  final String revision;
  final bool dirty;
  final bool canUndo;
  final bool canRedo;

  /// Committed transactions in the history.
  final int history;
  final String? readOnly;
}

/// One command's outcome inside an [Applied] transaction.
final class Mutation {
  Mutation.fromJson(this.json)
      : commandId = json['commandId'] as String? ?? '',
        command = json['command'] as String? ?? '',
        affectedIds = _strings(json['affectedIds']),
        ok = json['ok'] == true,
        diagnostics = Diagnostic.listOf(json['diagnostics']);

  final Map<String, Object?> json;
  final String commandId;
  final String command;

  /// Entity, component and asset ids the command touched (new ids included).
  final List<String> affectedIds;
  final bool ok;
  final List<Diagnostic> diagnostics;
}

/// The record of one `apply`: committed when [ok], otherwise the document
/// was rolled back to the byte and the diagnostics say why.
final class Applied {
  Applied.fromJson(this.json)
      : ok = json['ok'] == true,
        transactionId = json['transactionId'] as String? ?? '',
        baseRevision = json['baseRevision'] as String? ?? '',
        revision = json['revision'] as String? ?? '',
        mutations = [for (final m in json['mutations'] as List? ?? const []) Mutation.fromJson(_map(m))],
        diagnostics = Diagnostic.listOf(json['diagnostics']),
        state = json['state'] is Map<String, Object?> ? DocumentState.fromJson(_map(json['state'])) : null;

  final Map<String, Object?> json;
  final bool ok;
  final String transactionId;
  final String baseRevision;

  /// Revision after the transaction (equals [baseRevision] when refused).
  final String revision;
  final List<Mutation> mutations;

  /// Transaction-level diagnostics (a stale revision, a validation error).
  final List<Diagnostic> diagnostics;

  /// The document's state after the call.
  final DocumentState? state;

  /// The transaction was refused because the document moved on since the
  /// `baseRevision` the caller named (FYN-E0001); nothing was touched.
  bool get isConflict => !ok && diagnostics.any((d) => d.code == 'FYN-E0001');

  /// Every id any command touched, in command order, without repeats.
  List<String> get affectedIds => {for (final m in mutations) ...m.affectedIds}.toList();

  /// Every diagnostic, the transaction's and each command's.
  List<Diagnostic> get allDiagnostics => [...diagnostics, for (final m in mutations) ...m.diagnostics];
}

/// An undo or redo: the transaction replayed and the state after it.
final class Replayed {
  Replayed.fromJson(this.json)
      : transactionId = json['transactionId'] as String? ?? '',
        baseRevision = json['baseRevision'] as String? ?? '',
        revision = json['revision'] as String? ?? '',
        commands = _strings(json['commands']),
        affectedIds = _strings(json['affectedIds']),
        state = DocumentState.fromJson(_map(json['state']));

  final Map<String, Object?> json;
  final String transactionId;
  final String baseRevision;

  /// The document's revision now.
  final String revision;

  /// Command names of the replayed transaction.
  final List<String> commands;
  final List<String> affectedIds;
  final DocumentState state;
}

/// A rescan of the asset database.
final class RescanReport {
  RescanReport.fromJson(this.json)
      : assetCount = (json['assetCount'] as num?)?.toInt() ?? 0,
        scanned = _strings(json['scanned']),
        diagnostics = Diagnostic.listOf(json['diagnostics']);

  final Map<String, Object?> json;
  final int assetCount;

  /// The paths whose reference index entries were refreshed.
  final List<String> scanned;
  final List<Diagnostic> diagnostics;
}

/// `fynkel validate --json` over the open project.
final class ProjectReport {
  ProjectReport.fromJson(this.json)
      : ok = json['ok'] == true,
        errors = (json['errors'] as num?)?.toInt() ?? 0,
        warnings = (json['warnings'] as num?)?.toInt() ?? 0,
        documents = (json['documents'] as num?)?.toInt() ?? 0,
        sources = (json['sources'] as num?)?.toInt() ?? 0,
        references = (json['references'] as num?)?.toInt() ?? 0,
        diagnostics = Diagnostic.listOf(json['diagnostics']);

  final Map<String, Object?> json;
  final bool ok;
  final int errors;
  final int warnings;

  /// Documents, import sources and asset references the validation walked.
  final int documents;
  final int sources;
  final int references;
  final List<Diagnostic> diagnostics;
}
