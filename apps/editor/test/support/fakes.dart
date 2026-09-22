// Fakes for the shell tests: a project without the native runtime, a
// toolchain, a process runner, a directory chooser.
import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/documents/editor_document.dart';
import 'package:fynkel_editor/src/env/tools.dart';
import 'package:fynkel_editor/src/play/play_session.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'fake_document.dart';
import 'fake_process.dart';

export 'fake_document.dart';
export 'fake_process.dart';
export 'fake_preview.dart';

final class FakeProject extends EditorProject {
  FakeProject({required this.dir, this.name = 'Fake Game', this.readOnly, List<Diagnostic> diagnostics = const []})
    : _diagnostics = List.of(diagnostics);

  @override
  final String dir;
  @override
  final String name;
  @override
  final String? readOnly;
  List<Diagnostic> _diagnostics;
  @override
  List<Diagnostic> get diagnostics => List.unmodifiable(_diagnostics);
  @override
  bool validating = false;
  @override
  DateTime? validatedAt;
  @override
  String? validationError;
  @override
  int documents = 3;
  @override
  int sources = 2;
  @override
  int references = 4;
  @override
  int assetCount = 5;
  @override
  MigrationPreview? migrations;
  int revalidations = 0;
  bool closed = false;

  /// Documents by id and by path; `openDocument` looks both up.
  final fakeDocuments = <FakeDocument>[];

  /// Component types the project "knows" (schemas).
  List<ComponentType> types = const [
    ComponentType(type: 'fynkel.Transform', authored: true, allowed: true),
    ComponentType(type: 'fynkel.Camera', authored: true, allowed: true),
    ComponentType(type: 'fynkel.Light', authored: true, allowed: true),
    ComponentType(type: 'fynkel.MeshRenderer', authored: true, allowed: true),
    ComponentType(type: 'fynkel.Skin', authored: false, allowed: true),
    ComponentType(type: 'fynkel.AudioSource', authored: true, allowed: false),
  ];

  /// Defaults per native type (null = not authorable).
  Map<String, Map<String, Object?>> defaults = {
    'fynkel.Transform': {
      'position': [0, 0, 0],
      'rotation': [0, 0, 0, 1],
      'scale': [1, 1, 1],
    },
    'fynkel.Camera': {'projection': 'perspective', 'fov': 60, 'near': 0.1, 'far': 1000},
    'fynkel.Light': {
      'kind': 'point',
      'color': [1, 1, 1],
      'intensity': 1,
    },
    'fynkel.MeshRenderer': {'mesh': null, 'material': null},
  };

  final fileChangesController = StreamController<List<String>>.broadcast();
  final opened = <String>[];

  set diagnostics(List<Diagnostic> d) {
    _diagnostics = List.of(d);
    notifyListeners();
  }

  FakeDocument addDocument(FakeDocument d) {
    fakeDocuments.add(d);
    d.expander ??= (doc, anchor) => fakeExpand(
      doc,
      anchor,
      prefabs: prefabDocument,
      handleOf: (p) => openedHandles.contains(p.id) ? fakeDocuments.indexOf(p) + 1 : null,
    );
    return d;
  }

  /// The prefab document an instance's `{asset, sub}` resolves to: a
  /// prefab-kind document by id, or a fake imported hierarchy (a document
  /// with [FakeDocument.sub] `prefab` whose id is the source's).
  FakeDocument? prefabDocument(String asset, String? sub) {
    for (final d in fakeDocuments) {
      if (d.kind != 'prefab') continue;
      if (sub == null && d.sub == null && d.id == asset) return d;
      if (sub != null && d.sub == sub && (d.id == asset || d.sourceId == asset)) return d;
    }
    return null;
  }

  /// Ids `openDocument` was asked for (the fake session's "open" set, for
  /// `ExpansionPrefab.handle`).
  final openedHandles = <String>{};

  @override
  EditorDocument openDocument(String pathOrId) {
    opened.add(pathOrId);
    final hash = pathOrId.indexOf('#');
    if (hash >= 0) {
      final asset = pathOrId.substring(0, hash);
      final sub = pathOrId.substring(hash + 1);
      for (final d in fakeDocuments) {
        if (d.sub == sub && (d.id == asset || d.sourceId == asset || d.path == asset || d.relativePath == asset)) {
          openedHandles.add(d.id);
          return d;
        }
      }
      throw AuthoringException('openDocument', AuthoringStatus.loadFailed, [
        Diagnostic(code: 'FYN-D0401', severity: Severity.error, message: 'no imported hierarchy $pathOrId'),
      ]);
    }
    for (final d in fakeDocuments) {
      if (d.sub != null) continue;
      if (d.id == pathOrId || d.path == pathOrId || d.relativePath == pathOrId) {
        openedHandles.add(d.id);
        return d;
      }
    }
    throw AuthoringException('openDocument', AuthoringStatus.loadFailed, [
      Diagnostic(code: 'FYN-D0900', severity: Severity.error, message: 'no document $pathOrId'),
    ]);
  }

  @override
  List<DocumentAsset> documentAssets() => [
    for (final d in fakeDocuments) DocumentAsset(id: d.id, path: d.relativePath ?? d.path, name: d.name, kind: d.kind),
  ];

  @override
  List<ComponentType> componentTypes() => types;

  @override
  Map<String, Object?>? componentDefaults(String type) => defaults[type];

  /// Schemas by type; tests fill them (`fakeSchemas()` gives a usable set).
  List<TypeSchema> schemas = [];
  Map<String, Object?>? metadata;
  List<AssetEntry> assetEntries = [];

  @override
  List<TypeSchema> typeSchemas() => schemas;

  @override
  Map<String, Object?>? behaviourMetadata() => metadata;

  @override
  List<AssetEntry> assets() => assetEntries;

  // ── Asset operations, in memory (item 5) ──

  @override
  String get assetsDir => '$dir/assets';

  @override
  int assetsRevision = 0;

  /// Folders the fake "has" besides the ones `assetEntries` imply.
  List<String> extraFolders = [];

  /// References per asset id, as the `dependents` query answers them.
  Map<String, List<Map<String, Object?>>> fakeDependents = {};

  /// Sidecar settings per source id.
  Map<String, Map<String, Object?>> settings = {};

  /// Material fields per material id (`base_color` → a swatch).
  Map<String, Map<String, Object?>> materialFields = {};

  /// Every operation asked, `(kind, args)`.
  final operations = <(String, Map<String, Object?>)>[];
  final _undo = <(String, void Function())>[];
  final _redo = <(String, void Function())>[];
  int _fresh = 0;

  /// What `reimport` answers: null → ok with a summary; a message → refused.
  String? reimportRefusal;

  @override
  List<String> folders() {
    final out = <String>{...extraFolders};
    for (final e in assetEntries) {
      final parts = e.path.split('/');
      for (var i = 1; i < parts.length; i++) {
        out.add(parts.sublist(0, i).join('/'));
      }
    }
    return out.toList()..sort((a, b) => a.toLowerCase().compareTo(b.toLowerCase()));
  }

  @override
  String? createFolder(String rel) {
    extraFolders.add(rel);
    assetsRevision++;
    notifyListeners();
    return null;
  }

  AssetEntry? entryOf(String id) => assetEntries.where((e) => e.id == id).firstOrNull;

  @override
  AssetDetail? assetDetail(String id) {
    final e = entryOf(id);
    if (e == null) return null;
    return AssetDetail.fromJson({
      'id': e.id,
      'path': e.path,
      'name': e.name,
      'kind': e.kind,
      'document': ?e.document,
      'importer': ?e.importer,
      if (e.importer != null) 'importerVersion': 1,
      'file': {'size': 1234, 'modified': 1758450000.0},
      if (e.isSource)
        'sidecar': {
          'path': '${e.path}.meta.json',
          'importer': e.importer,
          'importerVersion': 1,
          'settings': settings[id] ?? const <String, Object?>{},
          'subAssets': e.subAssets,
        },
      if (e.document == 'material')
        'material': {
          'shader': 'fynkel.pbr@1',
          'alphaMode': 'opaque',
          'alphaCutoff': 0.5,
          'doubleSided': false,
          'fields': materialFields[id] ?? const <String, Object?>{},
        },
      if (e.document == 'scene' || e.document == 'prefab') 'entities': 3,
      'dependents': fakeDependents[id] ?? const [],
    });
  }

  @override
  List<Map<String, Object?>> dependents(String id) => fakeDependents[id] ?? const [];

  @override
  AssetHistory assetHistory() => AssetHistory.fromJson({
    'canUndo': _undo.isNotEmpty,
    'canRedo': _redo.isNotEmpty,
    'undoLabel': _undo.lastOrNull?.$1,
    'redoLabel': _redo.lastOrNull?.$1,
    'undo': [for (final u in _undo) u.$1],
    'redo': [for (final r in _redo) r.$1],
  });

  Never _refuse(String code, String message, {List<Map<String, Object?>> related = const []}) =>
      throw AuthoringException('operate', AuthoringStatus.invalidArgument, [
        Diagnostic(
          code: code,
          severity: Severity.error,
          message: message,
          related: [
            for (final r in related)
              Diagnostic(
                code: 'FYN-A0004',
                severity: Severity.error,
                message: '${r['path']} references $message',
                document: '$assetsDir/${r['path']}',
                pointer: r['pointer'] as String?,
                id: r['document'] as String?,
              ),
          ],
        ),
      ]);

  void _replace(String id, AssetEntry e) {
    final i = assetEntries.indexWhere((x) => x.id == id);
    if (i < 0) {
      assetEntries.add(e);
    } else {
      assetEntries[i] = e;
    }
  }

  Map<String, Object?> _entryJson(AssetEntry e) => {
    'id': e.id,
    'path': e.path,
    'name': e.name,
    'kind': e.kind,
    'document': ?e.document,
    'importer': ?e.importer,
    if (e.subAssets.isNotEmpty) 'subAssets': e.subAssets,
  };

  OperationResult _done(String op, String label, AssetEntry? entry, Map<String, Object?> extra) {
    assetsRevision++;
    notifyListeners();
    final h = assetHistory().json;
    return OperationResult.fromJson({
      'ok': true,
      'op': op,
      'label': label,
      'entry': entry == null ? null : _entryJson(entry),
      'assetCount': assetEntries.length,
      ...h,
      ...extra,
    });
  }

  @override
  OperationResult operate(String kind, [Map<String, Object?> args = const {}]) {
    operations.add((kind, args));
    if (readOnly != null) {
      throw AuthoringException('operate', AuthoringStatus.commandsRejected, [
        Diagnostic(code: 'FYN-E0007', severity: Severity.error, message: 'the project is read-only: $readOnly'),
      ]);
    }
    switch (kind) {
      case 'newDocument':
        final docKind = args['kind'] as String;
        var path = args['path'] as String;
        final suffix = '.$docKind.json';
        if (!path.endsWith(suffix)) path = '$path$suffix';
        if (assetEntries.any((e) => e.path == path)) _refuse('FYN-T0003', 'newDocument: $path exists');
        final stem = path.split('/').last;
        final name = args['name'] as String? ?? stem.substring(0, stem.length - suffix.length);
        final e = AssetEntry(
          id: 'new-${++_fresh}'.padRight(36, '0'),
          path: path,
          name: name,
          kind: 'document',
          document: docKind,
        );
        assetEntries.add(e);
        final label = 'New $docKind $stem';
        _redo.clear();
        _undo.add((label, () => assetEntries.removeWhere((x) => x.id == e.id)));
        return _done(kind, label, e, {'path': assetFile(path), 'id': e.id});
      case 'renameAsset':
        final id = args['asset'] as String;
        final e = entryOf(id) ?? _refuse('FYN-A0004', 'no asset $id');
        var to = args['path'] as String;
        final fileName = e.path.split('/').last;
        if (to.endsWith('/') || folders().contains(to)) {
          to = to.endsWith('/') ? '$to$fileName' : '$to/$fileName';
        } else if (!e.isSource) {
          final suffix = '.${e.document}.json';
          if (!to.endsWith(suffix)) {
            if (to.endsWith('.json')) _refuse('FYN-T0003', 'renameAsset: a ${e.document} keeps the $suffix suffix');
            to = '$to$suffix';
          }
        } else if (!to.contains('.')) {
          to = '$to.${fileName.split('.').last}';
        }
        if (to.startsWith('/')) to = to.substring(1);
        if (assetEntries.any((x) => x.path == to)) _refuse('FYN-T0003', 'renameAsset: $to exists');
        final newName = !e.isSource && e.name == fileName.substring(0, fileName.length - '.${e.document}.json'.length)
            ? to.split('/').last.replaceFirst('.${e.document}.json', '')
            : e.name;
        final moved = AssetEntry(
          id: e.id,
          path: to,
          name: newName,
          kind: e.kind,
          document: e.document,
          importer: e.importer,
          subAssets: e.subAssets,
        );
        _replace(id, moved);
        final label = to.split('/').last == fileName
            ? 'Move $fileName → ${to.substring(0, to.length - fileName.length)}'
            : 'Rename $fileName → ${to.split('/').last}';
        _redo.clear();
        _undo.add((label, () => _replace(id, e)));
        return _done(kind, label, moved, {'from': assetFile(e.path), 'to': assetFile(to)});
      case 'deleteAsset':
        final id = args['asset'] as String;
        final e = entryOf(id) ?? _refuse('FYN-A0004', 'no asset $id');
        final related = fakeDependents[id] ?? const [];
        if (related.isNotEmpty && args['force'] != true) {
          _refuse(
            'FYN-T0003',
            'deleteAsset: ${e.path} is referenced by ${related.length} document field${related.length == 1 ? '' : 's'}',
            related: related,
          );
        }
        if (fakeDocuments.any((d) => d.path == '$assetsDir/${e.path}')) {
          _refuse('FYN-T0003', 'deleteAsset: ${e.path} is open as a document — close its tab first');
        }
        assetEntries.removeWhere((x) => x.id == id);
        final label = 'Delete ${e.path.split('/').last}';
        _redo.clear();
        _undo.add((label, () => assetEntries.add(e)));
        return _done(kind, label, null, {
          'path': e.path,
          'trash': '$dir/.fynkel/trash/$id/${e.path.split('/').last}',
          'diagnostics': [
            for (final r in related)
              {'code': 'FYN-A0004', 'severity': 'warning', 'message': '${r['path']} now dangles'},
          ],
        });
      case 'setImportSettings':
        final id = args['asset'] as String;
        final e = entryOf(id) ?? _refuse('FYN-A0004', 'no asset $id');
        if (!e.isSource) _refuse('FYN-T0003', 'setImportSettings: ${e.path} is a document');
        final next = args['settings'];
        if (next is! Map<String, Object?>) _refuse('FYN-T0003', 'setImportSettings: settings must be an object');
        final before = settings[id] ?? const <String, Object?>{};
        settings[id] = Map.of(next);
        final label = 'Import settings of ${e.path.split('/').last}';
        _redo.clear();
        _undo.add((label, () => settings[id] = before));
        return _done(kind, label, e, {'settings': next, 'sidecar': '${e.path}.meta.json'});
      case 'reimport':
        final id = args['asset'] as String;
        final e = entryOf(id) ?? _refuse('FYN-A0004', 'no asset $id');
        if (!e.isSource) _refuse('FYN-T0003', 'reimport: ${e.path} is a document');
        if (reimportRefusal != null) {
          return OperationResult.fromJson({
            'ok': false,
            'op': kind,
            'path': e.path,
            'diagnostics': [
              {'code': 'FYN-A0003', 'severity': 'error', 'message': reimportRefusal},
            ],
          });
        }
        return OperationResult.fromJson({
          'ok': true,
          'op': kind,
          'label': 'Reimport ${e.path.split('/').last}',
          'entry': _entryJson(e),
          'assetCount': assetEntries.length,
          'summary': {'importer': e.importer},
          'subAssets': e.subAssets,
          'sidecarWritten': false,
          ...assetHistory().json,
        });
      case 'undoAsset' || 'redoAsset':
        final from = kind == 'undoAsset' ? _undo : _redo;
        if (from.isEmpty) {
          throw AuthoringException('operate', AuthoringStatus.commandsRejected, [
            Diagnostic(code: 'FYN-E0009', severity: Severity.error, message: 'nothing to ${kind.substring(0, 4)}'),
          ]);
        }
        final (label, inverse) = from.removeLast();
        // A snapshot swap: the inverse of the inverse is the state before it ran.
        final snapshot = List.of(assetEntries);
        final settingsSnapshot = {for (final s in settings.entries) s.key: Map.of(s.value)};
        inverse();
        (kind == 'undoAsset' ? _redo : _undo).add((
          label,
          () {
            assetEntries
              ..clear()
              ..addAll(snapshot);
            settings
              ..clear()
              ..addAll(settingsSnapshot);
          },
        ));
        return _done(kind, '${kind == 'undoAsset' ? 'Undo' : 'Redo'} $label', null, {});
      default:
        throw AuthoringException('operate', AuthoringStatus.invalidArgument, [
          Diagnostic(code: 'FYN-T0004', severity: Severity.error, message: 'unknown operation $kind'),
        ]);
    }
  }

  @override
  String? fileRevision(String path) => File(path).existsSync() ? 'disk-${File(path).lengthSync()}' : null;

  @override
  Stream<List<String>> get fileChanges => fileChangesController.stream;

  /// The watcher saw these paths.
  void filesChanged(List<String> paths) => fileChangesController.add(paths);

  @override
  Future<void> revalidate() async {
    revalidations++;
    validatedAt = DateTime(2026, 9, 21, 12, 30, 15);
    notifyListeners();
  }

  @override
  Future<void> close() async {
    closed = true;
  }
}

Diagnostic diag(
  String code, {
  Severity severity = Severity.error,
  String message = 'something is wrong',
  String? document,
  String? pointer,
  int? line,
  int? column,
  String? suggestion,
}) => Diagnostic(
  code: code,
  severity: severity,
  message: message,
  document: document,
  pointer: pointer,
  line: line,
  column: column,
  suggestion: suggestion,
);

Toolchain fakeToolchain({
  String? cli = '/fake/fynkel',
  String? root = '/fake/root',
  String? flutter = '/fake/flutter',
}) => Toolchain(
  fynkelCli: cli,
  fynkelRoot: root,
  flutter: flutter,
  flutterVersion: flutter == null ? null : '3.38.0',
  ideExecutable: '/usr/local/bin/zed',
  ideCommand: 'zed {file}:{line}:{column}',
  abiVersion: 1,
  abiError: null,
);

/// Records every command line; answers with the canned result.
final class RecordingRunner {
  RecordingRunner({this.exitCode = 0, this.stdout = '', this.stderr = ''});

  int exitCode;
  String stdout;
  String stderr;
  final calls = <(String, List<String>, String?)>[];

  Future<ProcessResult> call(String executable, List<String> args, {String? workingDirectory}) async {
    calls.add((executable, args, workingDirectory));
    return ProcessResult(1, exitCode, stdout, stderr);
  }
}

/// The default for a widget test: Play must not reach a real process.
Future<Process> _noPlayProcess(
  String executable,
  List<String> arguments, {
  String? workingDirectory,
  Map<String, String>? environment,
}) async => throw StateError('this test did not expect Play to start $executable');

/// Hands Play a [FakePlayProcess] and remembers how it was asked for.
final class FakePlayLauncher {
  final calls = <(String, List<String>, String?, Map<String, String>?)>[];
  final processes = <FakePlayProcess>[];

  /// The process of the last Play (the test feeds it channel records).
  FakePlayProcess get last => processes.last;

  Future<Process> call(
    String executable,
    List<String> arguments, {
    String? workingDirectory,
    Map<String, String>? environment,
  }) async {
    calls.add((executable, arguments, workingDirectory, environment));
    final p = FakePlayProcess(pid: 7000 + processes.length);
    processes.add(p);
    return p;
  }
}

/// Creates a directory that passes `isFynkelProject` (a project file only).
Directory fakeProjectDir(String name) {
  final dir = Directory.systemTemp.createTempSync('fynkel_editor_$name');
  File('${dir.path}/fynkel.project.json').writeAsStringSync('{"name":"$name"}');
  return dir;
}

/// The shell with everything faked. [projects] maps a directory to the
/// project [openProject] returns for it.
Widget shellApp({
  Key? key,
  required Preferences prefs,
  LayoutStore? store,
  Map<String, EditorProject> projects = const {},
  String? chosenDirectory,
  Toolchain? toolchain,
  RecordingRunner? runner,
  VoidCallback? quit,
  String? initialProject,
  Future<String?> Function(String suggestedName)? chooseSavePath,
  StartPlayProcess? startPlayProcess,
  bool sceneViewport = false,
}) {
  return FynkelEditorApp(
    key: key,
    prefs: prefs,
    layoutStore: store ?? MemoryLayoutStore(),
    openProject: (dir) async {
      final p = projects[dir];
      if (p == null) {
        throw AuthoringException('openProject', AuthoringStatus.loadFailed, [
          Diagnostic(code: 'FYN-A0001', severity: Severity.error, message: 'no fake project at $dir'),
        ]);
      }
      return p;
    },
    chooseDirectory: (_, {title, initialDirectory}) async => chosenDirectory,
    chooseSavePath: chooseSavePath ?? (name) async => null,
    // Widget tests have no Metal texture to draw into; the Scene view's
    // model still runs over the fake preview through the painted layer.
    sceneViewport: sceneViewport,
    detectToolchain: (_) async => toolchain ?? fakeToolchain(),
    run: runner?.call ?? RecordingRunner().call,
    startPlayProcess: startPlayProcess ?? _noPlayProcess,
    nativeMenuBar: false,
    quit: quit ?? () {},
    initialProject: initialProject,
  );
}

/// A large window, so menus and three columns of panels fit.
Future<void> pumpLarge(WidgetTester tester, Widget app) async {
  tester.view.physicalSize = const Size(1600, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(app);
  await tester.pumpAndSettle();
}
