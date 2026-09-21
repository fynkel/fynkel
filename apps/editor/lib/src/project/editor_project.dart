// The open project as the shell sees it: a name, a directory, whether it is
// read-only and why, and its diagnostics. `SessionProject` is the real one
// over `AuthoringSession`; the abstract class lets widget tests run the
// shell without the native runtime.
import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;
import 'package:watcher/watcher.dart';

abstract class EditorProject extends ChangeNotifier {
  String get dir;
  String get name;

  /// Why the project is read-only (a lock written by a newer Fynkel), or null.
  String? get readOnly;
  bool get isReadOnly => readOnly != null;

  /// Open-time diagnostics plus the last whole-project validation, de-duplicated.
  List<Diagnostic> get diagnostics;
  bool get validating;
  DateTime? get validatedAt;

  /// The validation itself failed (an `AuthoringException`), as text.
  String? get validationError;
  int get documents;
  int get sources;
  int get references;
  int get assetCount;

  /// What `fynkel migrate` would do to the documents behind their format
  /// version (null before the first validation).
  MigrationPreview? get migrations;
  int get outdatedDocuments => migrations?.outdated ?? 0;

  int get errorCount => diagnostics.where((d) => d.severity == Severity.error).length;
  int get warningCount => diagnostics.where((d) => d.severity == Severity.warning).length;

  Future<void> revalidate();
  Future<void> close();

  /// A project path shown to the user: relative to the project directory.
  String relative(String path) => p.isWithin(dir, path) || p.equals(dir, path) ? p.relative(path, from: dir) : path;
}

/// The project over the authoring session, with a directory watcher that
/// rescans changed asset paths and revalidates (debounced).
final class SessionProject extends EditorProject {
  SessionProject._(this.session, this._info, {required this.dir});

  /// Opens [dir]; throws `AuthoringException` when it is not a project.
  /// [watch] false turns the file watcher off (tests).
  static Future<SessionProject> open(String dir, {bool watch = true, AuthoringSession? session}) async {
    final s = session ?? AuthoringSession();
    final ProjectInfo info;
    try {
      info = s.openProject(dir);
    } catch (_) {
      if (session == null) s.dispose();
      rethrow;
    }
    final project = SessionProject._(s, info, dir: info.dir.isEmpty ? dir : info.dir);
    await project.revalidate();
    if (watch) project._watch();
    return project;
  }

  final AuthoringSession session;
  ProjectInfo _info;
  ProjectReport? _report;
  bool _validating = false;
  DateTime? _validatedAt;
  String? _validationError;
  StreamSubscription<WatchEvent>? _watcher;
  Timer? _debounce;
  final _pending = <String>{};
  bool _closed = false;

  @override
  final String dir;
  @override
  String get name => _info.name.isEmpty ? p.basename(dir) : _info.name;
  @override
  String? get readOnly => _info.readOnly;
  @override
  bool get validating => _validating;
  @override
  DateTime? get validatedAt => _validatedAt;
  @override
  String? get validationError => _validationError;
  @override
  int get documents => _report?.documents ?? 0;
  @override
  int get sources => _report?.sources ?? 0;
  @override
  int get references => _report?.references ?? 0;
  @override
  int get assetCount => _info.assetCount;
  ProjectInfo get info => _info;
  ProjectReport? get report => _report;
  MigrationPreview? _migrations;
  @override
  MigrationPreview? get migrations => _migrations;

  @override
  List<Diagnostic> get diagnostics {
    final seen = <String>{};
    final out = <Diagnostic>[];
    for (final d in [..._info.diagnostics, ...?_report?.diagnostics]) {
      if (seen.add('${d.code}|${d.document}|${d.pointer}|${d.message}')) out.add(d);
    }
    return out;
  }

  /// Runs the whole-project validation `fynkel validate` runs.
  @override
  Future<void> revalidate() async {
    if (_closed) return;
    _validating = true;
    notifyListeners();
    // Let the frame that shows "validating" paint before the synchronous call.
    await Future<void>.delayed(Duration.zero);
    if (_closed) return;
    try {
      _report = session.validateProject();
      _migrations = session.migrationPreview();
      _validationError = null;
    } on AuthoringException catch (e) {
      _validationError = e.toString();
    }
    _validating = false;
    _validatedAt = DateTime.now();
    notifyListeners();
  }

  static const _ignoredDirs = {'.fynkel', '.dart_tool', 'build', 'fynkel_cache', '.git', 'ios', 'macos', '.idea'};

  void _watch() {
    try {
      _watcher = DirectoryWatcher(dir).events.listen(_onEvent, onError: (_) {});
    } catch (_) {
      _watcher = null;
    }
  }

  void _onEvent(WatchEvent event) {
    final rel = p.split(relative(event.path));
    if (rel.any(_ignoredDirs.contains)) return;
    _pending.add(event.path);
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 400), _flush);
  }

  Future<void> _flush() async {
    if (_closed) return;
    final changed = _pending.toList();
    _pending.clear();
    final assetsDir = _info.assetsDir.isEmpty ? p.join(dir, 'assets') : _info.assetsDir;
    final assets = changed.where((c) => p.isWithin(assetsDir, c)).toList();
    try {
      if (assets.isNotEmpty) session.rescan(assets);
      _info = session.refreshProject();
    } on AuthoringException catch (e) {
      _validationError = e.toString();
    }
    await revalidate();
  }

  @override
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    _debounce?.cancel();
    await _watcher?.cancel();
    try {
      session.closeProject();
    } catch (_) {
      // A poisoned session has nothing left to close.
    }
    session.dispose();
    notifyListeners();
  }

  @override
  void dispose() {
    if (!_closed) close();
    super.dispose();
  }
}

/// True when [dir] holds a project file (the launcher's Open check).
bool isFynkelProject(String dir) => File(p.join(dir, 'fynkel.project.json')).existsSync();
