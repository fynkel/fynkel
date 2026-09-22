// Typed views over the JSON the authoring ABI writes. Every model keeps the
// raw map in `json` so a panel can reach a field these classes do not name.

import 'dart:typed_data';

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
/// The `revision` query: a document file as it is on disk now.
final class FileRevision {
  FileRevision.fromJson(this.json)
      : path = json['path'] as String? ?? '',
        kind = json['kind'] as String? ?? '',
        id = json['id'] as String? ?? '',
        name = json['name'] as String? ?? '',
        formatVersion = (json['formatVersion'] as num?)?.toInt() ?? 0,
        revision = json['revision'] as String? ?? '';

  final Map<String, Object?> json;
  final String path;
  final String kind;
  final String id;
  final String name;
  final int formatVersion;

  /// blake3 of the canonical text — comparable with `DocumentState.revision`.
  final String revision;
}

/// The answer to `DocumentHandle.entities(ids)`: each asked-for entity's JSON
/// (`null` when the document has no such entity) and the root ids in order.
final class EntityPatch {
  const EntityPatch({required this.entities, required this.roots, required this.state});

  final Map<String, Map<String, Object?>?> entities;
  final List<String> roots;
  final DocumentState state;
}

final class Applied {
  Applied.fromJson(this.json)
      : ok = json['ok'] == true,
        transactionId = json['transactionId'] as String? ?? '',
        baseRevision = json['baseRevision'] as String? ?? '',
        revision = json['revision'] as String? ?? '',
        mutations = [for (final m in json['mutations'] as List? ?? const []) Mutation.fromJson(_map(m))],
        diagnostics = Diagnostic.listOf(json['diagnostics']),
        state = json['state'] is Map<String, Object?> ? DocumentState.fromJson(_map(json['state'])) : null,
        previews = PreviewSyncReport.listOf(json['previews']);

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

  /// The edit-preview worlds of the session brought up to date with this
  /// transaction (empty when none is open).
  final List<PreviewSyncReport> previews;

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
        state = DocumentState.fromJson(_map(json['state'])),
        previews = PreviewSyncReport.listOf(json['previews']);

  final Map<String, Object?> json;
  final String transactionId;
  final String baseRevision;

  /// The document's revision now.
  final String revision;

  /// Command names of the replayed transaction.
  final List<String> commands;
  final List<String> affectedIds;
  final DocumentState state;

  /// The edit-preview worlds brought up to date with the replay.
  final List<PreviewSyncReport> previews;
}

/// One step `fynkel migrate` knows: `kind` documents at `from` become `to`.
/// The registry lists it under `id`; a document's `applied[]` under `name`
/// (the same text, `scene/0-1/entities-keyed-by-id`).
final class MigrationStep {
  MigrationStep.fromJson(Map<String, Object?> json)
      : id = json['id'] as String? ?? json['name'] as String? ?? '',
        kind = json['kind'] as String? ?? '',
        from = (json['from'] as num?)?.toInt() ?? 0,
        to = (json['to'] as num?)?.toInt() ?? 0,
        description = json['description'] as String? ?? '';

  final String id;
  final String kind;
  final int from;
  final int to;
  final String description;
}

/// What `fynkel migrate --dry-run` would do to one outdated document.
final class DocumentMigration {
  DocumentMigration.fromJson(Map<String, Object?> json)
      : path = json['path'] as String? ?? '',
        name = json['name'] as String? ?? '',
        kind = json['kind'] as String? ?? '',
        from = (json['from'] as num?)?.toInt() ?? 0,
        to = (json['to'] as num?)?.toInt() ?? 0,
        ok = json['ok'] == true,
        applied = [
          for (final a in json['applied'] as List? ?? const [])
            if (a is Map) MigrationStep.fromJson(a.cast<String, Object?>()),
        ],
        diff = json['diff'] as String? ?? '',
        diagnostics = Diagnostic.listOf(json['diagnostics']);

  /// Absolute path of the document.
  final String path;
  final String name;
  final String kind;
  final int from;
  final int to;

  /// False when a step failed or the result does not validate; [diagnostics] say why.
  final bool ok;

  /// The steps, in order (`applied[i].id` like `scene/0-1/entities-keyed-by-id`).
  final List<MigrationStep> applied;

  /// Unified diff of the canonical text, empty when nothing would change.
  final String diff;
  final List<Diagnostic> diagnostics;
}

/// The `migrations` query: the documents behind their kind's `formatVersion`
/// and the steps this build knows. Nothing is written; `fynkel migrate`
/// does that, keeping `<file>.bak`.
final class MigrationPreview {
  MigrationPreview.fromJson(Map<String, Object?> json)
      : outdated = (json['outdated'] as num?)?.toInt() ?? 0,
        steps = [
          for (final s in json['steps'] as List? ?? const [])
            if (s is Map) MigrationStep.fromJson(s.cast<String, Object?>()),
        ],
        documents = [
          for (final d in json['documents'] as List? ?? const [])
            if (d is Map) DocumentMigration.fromJson(d.cast<String, Object?>()),
        ];

  final int outdated;
  final List<MigrationStep> steps;
  final List<DocumentMigration> documents;

  bool get isEmpty => documents.isEmpty;
}

/// A rescan of the asset database.
final class RescanReport {
  RescanReport.fromJson(this.json)
      : assetCount = (json['assetCount'] as num?)?.toInt() ?? 0,
        scanned = _strings(json['scanned']),
        diagnostics = Diagnostic.listOf(json['diagnostics']),
        previews = PreviewSyncReport.listOf(json['previews']);

  final Map<String, Object?> json;
  final int assetCount;

  /// The paths whose reference index entries were refreshed.
  final List<String> scanned;
  final List<Diagnostic> diagnostics;

  /// The edit-preview worlds re-described over the rescanned database (a
  /// prefab changed on disk reaches the scenes that instance it).
  final List<PreviewSyncReport> previews;
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

/// The asset operations' undo/redo state (`assetHistory` query, and on
/// every [OperationResult]) — the Project browser's Edit menu.
final class AssetHistory {
  AssetHistory.fromJson(this.json)
      : canUndo = json['canUndo'] == true,
        canRedo = json['canRedo'] == true,
        undoLabel = json['undoLabel'] as String?,
        redoLabel = json['redoLabel'] as String?,
        undo = _strings(json['undo']),
        redo = _strings(json['redo']);

  const AssetHistory.empty()
      : json = const {},
        canUndo = false,
        canRedo = false,
        undoLabel = null,
        redoLabel = null,
        undo = const [],
        redo = const [];

  final Map<String, Object?> json;
  final bool canUndo;
  final bool canRedo;

  /// What the next undo / redo would take back, as the menu shows it
  /// (`Rename robot.glb → bot.glb`), or null.
  final String? undoLabel;
  final String? redoLabel;

  /// Every label on each stack, the next one last.
  final List<String> undo;
  final List<String> redo;
}

/// What one asset operation (`operate`) answered: the affected entry
/// when it still exists, the history after it, the operation's own
/// fields in [json] (`path`, `from`/`to`, `trash`, `sidecar`, `settings`,
/// `summary`, `subAssets`, `sidecarWritten`) and any warnings.
final class OperationResult {
  OperationResult.fromJson(this.json)
      : ok = json['ok'] == true,
        op = json['op'] as String? ?? '',
        label = json['label'] as String? ?? '',
        entry = json['entry'] is Map<String, Object?> ? json['entry'] as Map<String, Object?> : null,
        assetCount = (json['assetCount'] as num?)?.toInt() ?? 0,
        history = AssetHistory.fromJson(json),
        diagnostics = Diagnostic.listOf(json['diagnostics']),
        previews = PreviewSyncReport.listOf(json['previews']);

  final Map<String, Object?> json;

  /// False only for a `reimport` the importer refused (see [diagnostics]);
  /// a refused operation throws instead.
  final bool ok;

  /// The edit-preview worlds brought up to date after the operation.
  final List<PreviewSyncReport> previews;
  final String op;

  /// The history label (`New material gold.material.json`).
  final String label;

  /// The asset's entry after the operation (`fynkel asset list --json`
  /// shape), or null when it no longer exists (a delete).
  final Map<String, Object?>? entry;
  final int assetCount;
  final AssetHistory history;

  /// Warnings (a forced delete's dependents) or the importer's refusal.
  final List<Diagnostic> diagnostics;

  /// The new path of a `newDocument`, `renameAsset` (`to`) or the
  /// trashed file's original path.
  String? get path => (json['to'] ?? json['path']) as String?;

  /// The id of a created asset.
  String? get id => json['id'] as String? ?? (entry?['id'] as String?);
}

/// The `assetDetail` query: one asset and what the Asset panel shows of it.
final class AssetDetail {
  AssetDetail.fromJson(this.json)
      : id = json['id'] as String? ?? '',
        path = json['path'] as String? ?? '',
        name = json['name'] as String? ?? '',
        kind = json['kind'] as String? ?? '',
        document = json['document'] as String?,
        importer = json['importer'] as String?,
        importerVersion = (json['importerVersion'] as num?)?.toInt(),
        size = (_map(json['file'])['size'] as num?)?.toInt(),
        modified = (_map(json['file'])['modified'] as num?)?.toDouble(),
        settings = _map(_map(json['sidecar'])['settings']),
        sidecarPath = _map(json['sidecar'])['path'] as String?,
        subAssets = {
          for (final e in _map(_map(json['sidecar'])['subAssets'] ?? json['subAssets']).entries)
            if (e.value is String) e.key: e.value as String
        },
        material = json['material'] is Map<String, Object?> ? json['material'] as Map<String, Object?> : null,
        entities = (json['entities'] as num?)?.toInt(),
        fields = json['fields'] is Map<String, Object?> ? json['fields'] as Map<String, Object?> : null,
        dependents = [
          for (final d in json['dependents'] as List? ?? const [])
            if (d is Map<String, Object?>) d
        ];

  final Map<String, Object?> json;
  final String id;

  /// Relative to the asset root, `/`-separated.
  final String path;
  final String name;

  /// `document` or `source`.
  final String kind;

  /// The document kind (`scene`, `prefab`, `material`, `rig`, `blend`) or null for a source.
  final String? document;

  /// The importer id of a source (`fynkel.image`), or null.
  final String? importer;
  final int? importerVersion;

  /// The file's size in bytes and modification time (seconds since the
  /// epoch), when it could be read.
  final int? size;
  final double? modified;

  /// A source's sidecar `settings` object (empty for a document).
  final Map<String, Object?> settings;
  final String? sidecarPath;

  /// Sub-asset id → uuid (a source's from its sidecar, a rig's derived).
  final Map<String, String> subAssets;

  /// A material's `{ shader, alphaMode, alphaCutoff, doubleSided, fields }`.
  final Map<String, Object?>? material;

  /// A scene's or prefab's entity count.
  final int? entities;

  /// A rig's or blend's whole root.
  final Map<String, Object?>? fields;

  /// Every reference to the asset: `{ document, path, pointer, asset, sub? }`.
  final List<Map<String, Object?>> dependents;

  bool get isSource => kind == 'source';
}

// ---- the edit-preview world (item 6, authoring ABI v3) ------------------

/// The report of one incremental sync of a preview against its document
/// (`Preview::sync`): what the diff did, and whether it fell back to a
/// fresh load.
final class PreviewSyncReport {
  PreviewSyncReport.fromJson(this.json)
      : handle = json['handle'] as int? ?? 0,
        spawned = json['spawned'] as int? ?? 0,
        destroyed = json['destroyed'] as int? ?? 0,
        updated = json['updated'] as int? ?? 0,
        reordered = json['reordered'] as int? ?? 0,
        steps = json['steps'] as int? ?? 0,
        reloaded = json['reloaded'] == true,
        diagnostics = Diagnostic.listOf(json['diagnostics']),
        describeDiagnostics = Diagnostic.listOf(json['describeDiagnostics']);

  final Map<String, Object?> json;

  /// The document handle the preview belongs to.
  final int handle;
  final int spawned, destroyed, updated, reordered;

  /// World steps the sync took (0: nothing to do).
  final int steps;

  /// The diff did not settle and the world was loaded fresh instead.
  final bool reloaded;

  /// The world's diagnostics of the sync (or of the fallback load).
  final List<Diagnostic> diagnostics;

  /// Diagnostics of describing the document (a dangling reference).
  final List<Diagnostic> describeDiagnostics;

  /// Nothing changed in the world.
  bool get isNoOp => steps == 0 && !reloaded;

  static List<PreviewSyncReport> listOf(Object? v) =>
      v is List ? [for (final r in v) if (r is Map<String, Object?>) PreviewSyncReport.fromJson(r)] : const [];
}

/// The counts the stats overlay shows.
final class PreviewStats {
  PreviewStats.fromJson(Map<String, Object?> json)
      : entities = json['entities'] as int? ?? 0,
        live = json['live'] as int? ?? 0,
        meshes = json['meshes'] as int? ?? 0,
        lights = json['lights'] as int? ?? 0,
        hidden = json['hidden'] as int? ?? 0,
        tick = json['tick'] as int? ?? 0,
        loads = json['loads'] as int? ?? 0,
        syncs = json['syncs'] as int? ?? 0;

  const PreviewStats.zero()
      : entities = 0,
        live = 0,
        meshes = 0,
        lights = 0,
        hidden = 0,
        tick = 0,
        loads = 0,
        syncs = 0;

  /// Entities of the document in the preview; live entities in the world
  /// (equal unless a load was refused).
  final int entities, live;

  /// Mesh instances and lights the picture has (hidden ones left out).
  final int meshes, lights;
  final int hidden;

  /// The world's tick: every step the preview took (never a game tick).
  final int tick;

  /// Fresh loads (1 after open; +1 per fallback, Reload or recook) and
  /// incremental syncs so far.
  final int loads, syncs;
}

/// The editor camera the picture is drawn through — in the scene camera's
/// place, never a game camera. [world] is the camera's world matrix, 16
/// numbers column-major (glam `Mat4::to_cols_array` order); the camera
/// looks along its −Z. [fovY] is in radians; the fields default to the
/// engine's `Camera` defaults (60°, 10 m, 0.1, 1000, auto exposure).
/// [manualExposure] is an EV100 that replaces the automatic exposure (a
/// scene camera's `exposure_mode: manual`, when the view is aligned to
/// one).
final class PreviewCamera {
  const PreviewCamera({
    required this.world,
    this.orthographic = false,
    this.fovY = 1.0471975511965976,
    this.orthoHeight = 10,
    this.near = 0.1,
    this.far = 1000,
    this.manualExposure,
  });

  final List<double> world;
  final bool orthographic;
  final double fovY, orthoHeight, near, far;
  final double? manualExposure;

  Map<String, Object?> toJson() => {
        'world': world,
        'projection': orthographic ? 'orthographic' : 'perspective',
        'fovY': fovY,
        'orthoHeight': orthoHeight,
        'near': near,
        'far': far,
        'exposureMode': manualExposure == null ? 'auto' : 'manual',
        if (manualExposure != null) 'exposure': manualExposure,
      };

  /// The camera at a viewport's [aspect] (width / height): what a pick or
  /// a marquee needs to turn a pixel into a ray.
  Map<String, Object?> viewJson(double aspect) => {...toJson(), 'aspect': aspect};
}

/// One entity a pick ray hit.
final class PreviewHit {
  PreviewHit.fromJson(Map<String, Object?> json)
      : id = json['id'] as String? ?? '',
        distance = (json['distance'] as num? ?? 0).toDouble(),
        point = _doubles(json['point']),
        mesh = json['mesh'] == true;

  final String id;

  /// Along the ray from its origin, metres.
  final double distance;

  /// World-space hit point `[x, y, z]`.
  final List<double> point;

  /// Hit on a mesh's triangle (false: the icon sphere of a light, camera
  /// or empty).
  final bool mesh;
}

/// The result of a pick: the ray that was cast and every entity it hit,
/// nearest first (a repeated click cycles through them).
final class PreviewPick {
  PreviewPick.fromJson(Map<String, Object?> json)
      : origin = _doubles(_map(json['ray'])['origin']),
        dir = _doubles(_map(json['ray'])['dir']),
        hits = [for (final h in json['hits'] as List? ?? const []) PreviewHit.fromJson(_map(h))];

  final List<double> origin, dir;
  final List<PreviewHit> hits;

  bool get isEmpty => hits.isEmpty;
  PreviewHit? get nearest => hits.isEmpty ? null : hits.first;
}

/// A local transform the gizmo previews: position `[x, y, z]`, rotation
/// `[x, y, z, w]` (unit), scale `[x, y, z]`.
final class PreviewTransform {
  const PreviewTransform({required this.id, required this.position, required this.rotation, required this.scale});

  final String id;
  final List<double> position, rotation, scale;

  Map<String, Object?> toJson() => {'id': id, 'position': position, 'rotation': rotation, 'scale': scale};
}

/// The lines a `lines` call returned: [count] segments as ten numbers each
/// — `ax ay az bx by bz r g b a` — flat in [data], ready for the viewport's
/// debug lines.
final class PreviewLines {
  PreviewLines.fromJson(Map<String, Object?> json)
      : count = json['count'] as int? ?? 0,
        data = Float32List.fromList([
          for (final line in json['lines'] as List? ?? const [])
            for (final v in line as List) (v as num).toDouble(),
        ]);

  final int count;
  final Float32List data;
}

/// The cook the first preview of a session ran, or a `recook` (the
/// `fynkel cook --json` report).
final class CookReport {
  CookReport.fromJson(this.json)
      : ok = json['ok'] == true,
        profile = json['profile'] as String? ?? '',
        cached = json['cached'] as int? ?? 0,
        cooked = json['cooked'] as int? ?? 0,
        failed = json['failed'] as int? ?? 0,
        diagnostics = Diagnostic.listOf(json['diagnostics']);

  final Map<String, Object?> json;

  /// No asset failed to cook.
  final bool ok;
  final String profile;
  final int cached, cooked, failed;
  final List<Diagnostic> diagnostics;
}

List<double> _doubles(Object? v) => v is List ? [for (final x in v) if (x is num) x.toDouble()] : const [];
