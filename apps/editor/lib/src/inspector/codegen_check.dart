// The "run `fynkel codegen`" banner's source of truth (blueprint §11.2,
// §19.2e item 4): whether the project's committed behaviour metadata
// (`lib/generated/behaviours.g.json` and `schemas/behaviours/`) is what the
// game's Dart source would generate now. Two signals, cheapest first:
//
//  1. **Committed drift** — the metadata's inline schemas and the schema
//     files disagree, or one names a type the other lacks. Computed from
//     what the session already read; instant; no process.
//  2. **The CLI check** — `fynkel codegen --check --project <dir> --json`,
//     one of the three commands the editor is allowed to run (ADR-0016
//     keeps game code and codegen out of the editor process; the CLI runs
//     `build_runner` in the game package and restores the committed files,
//     so a check never writes). It takes seconds; it runs in the background
//     at open and on request, never on a timer.
//
// The banner shows the command to run and a button to copy it. The
// editor runs the generating command itself — the CLI as a subprocess,
// never game code in the editor process (ADR-0016 amendments of
// 2026-09-24) — on Create → Behaviour ([generate]) and, as Unity
// recompiles, when a script under `lib/` is saved: the session then
// re-reads the metadata, so the Inspector shows a field the moment its
// script is saved. [watch] keeps `fynkel codegen --watch` running for
// that (about a second a save); a CLI without it gets one
// `fynkel codegen` per save instead ([recompileOnSave], 3 s and more).
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path/path.dart' as p;

import '../env/tools.dart';

enum CodegenStatus {
  /// Not checked yet.
  unknown,
  running,
  upToDate,

  /// Generated files differ from what the source would produce.
  stale,

  /// The check itself could not run (no CLI, no Dart SDK, build error).
  failed,

  /// The project is not a Dart package (no `pubspec.yaml`): nothing to check.
  notADartPackage,
}

/// One behaviour type whose committed metadata and schema file disagree.
final class CommittedDrift {
  const CommittedDrift(this.type, this.reason);

  final String type;

  /// `no schema file`, `not in behaviours.g.json`, `schema differs`.
  final String reason;

  @override
  String toString() => '$type: $reason';
}

/// Compares the metadata's `behaviours[].schema` with the schema files the
/// session loaded (`typeSchemas`): the instant staleness signal.
List<CommittedDrift> committedDrift(Map<String, Object?>? metadata, Map<String, Map<String, Object?>?> fileSchemas) {
  if (metadata == null) return const [];
  final out = <CommittedDrift>[];
  final inMetadata = <String>{};
  for (final b in metadata['behaviours'] as List? ?? const []) {
    if (b is! Map) continue;
    final type = b['id'] as String? ?? '';
    inMetadata.add(type);
    if (!fileSchemas.containsKey(type)) {
      out.add(CommittedDrift(type, 'no schema file'));
      continue;
    }
    final file = fileSchemas[type];
    if (file == null) {
      out.add(CommittedDrift(type, 'declared without a schema file'));
    } else if (jsonEncode(_normalize(b['schema'])) != jsonEncode(_normalize(file))) {
      out.add(CommittedDrift(type, 'schema differs'));
    }
  }
  for (final type in fileSchemas.keys) {
    if (!inMetadata.contains(type)) out.add(CommittedDrift(type, 'not in behaviours.g.json'));
  }
  out.sort((a, b) => a.type.compareTo(b.type));
  return out;
}

Object? _normalize(Object? v) => switch (v) {
  Map() => {for (final k in (v.keys.map((k) => k.toString()).toList()..sort())) k: _normalize(v[k])},
  List() => [for (final e in v) _normalize(e)],
  num() => v == v.roundToDouble() && v.abs() < 1e15 ? v.round() : v,
  _ => v,
};

/// Starts a long-running process (the `fynkel codegen --watch` one) —
/// injectable so tests never spawn one.
typedef StartProcess = Future<Process> Function(
  String executable,
  List<String> arguments, {
  String? workingDirectory,
  Map<String, String>? environment,
});

Future<Process> startProcess(
  String executable,
  List<String> arguments, {
  String? workingDirectory,
  Map<String, String>? environment,
}) => Process.start(executable, arguments, workingDirectory: workingDirectory, environment: environment);

final class CodegenCheck extends ChangeNotifier {
  CodegenCheck({
    required this.projectDir,
    required this.run,
    required this.cli,
    required this.repoRoot,
    this.start = startProcess,
  });

  /// The game project (the Dart package with `pubspec.yaml`).
  final String projectDir;
  final RunProcess run;

  /// The `fynkel` executable, or null when none is configured.
  final String? Function() cli;

  /// The Fynkel checkout the CLI's native pipeline needs (`--root`), or null.
  final String? Function() repoRoot;

  /// Starts the [watch] process.
  final StartProcess start;

  CodegenStatus status = CodegenStatus.unknown;

  /// Whether the running command writes ([generate]) rather than checks.
  bool generating = false;
  List<String> stalePaths = const [];
  String? error;
  DateTime? checkedAt;
  int runs = 0;

  bool get isDartPackage => File(p.join(projectDir, 'pubspec.yaml')).existsSync();

  /// What to run to fix it (shown and copied; [generate] runs its
  /// project-only form).
  String get command => 'fynkel codegen --project ${_quote(projectDir)}';

  /// The command line the check runs, for the log and the tests; with
  /// [check] false the one [generate] runs, which writes the game's files
  /// only (`--project-only`: never the engine's own generated files).
  List<String> arguments({bool check = true}) => [
    'codegen',
    if (check) '--check',
    '--project',
    projectDir,
    if (!check) '--project-only',
    if (check && repoRoot() != null) ...['--root', repoRoot()!],
    '--json',
  ];

  Future<void> check() async {
    // The watch reports every build: its status is the check's answer.
    if (watching) return;
    if (!isDartPackage) {
      status = CodegenStatus.notADartPackage;
      notifyListeners();
      return;
    }
    final exe = cli();
    if (exe == null) {
      status = CodegenStatus.failed;
      error = 'no `fynkel` CLI is configured (Preferences → Fynkel CLI)';
      notifyListeners();
      return;
    }
    status = CodegenStatus.running;
    error = null;
    notifyListeners();
    runs++;
    try {
      final r = await run(exe, arguments(), workingDirectory: projectDir);
      if (_disposed) return;
      _read(r);
    } catch (e) {
      if (_disposed) return;
      status = CodegenStatus.failed;
      error = 'could not run `fynkel codegen --check`: $e';
    }
    checkedAt = DateTime.now();
    notifyListeners();
  }

  /// Regenerates the behaviour metadata and schemas (`fynkel codegen
  /// --project`, which writes). Returns the problem, or null when the files
  /// were written or already up to date; the status follows either way.
  Future<String?> generate() async {
    if (!isDartPackage) return 'the project is not a Dart package (no pubspec.yaml)';
    final exe = cli();
    if (exe == null) return 'no `fynkel` CLI is configured (Preferences → Fynkel CLI)';
    // The watch sees the new file itself; a second build_runner would fight it.
    if (watching) {
      return _nextBuild().timeout(const Duration(minutes: 2), onTimeout: () => 'the script watch did not answer');
    }
    status = CodegenStatus.running;
    generating = true;
    error = null;
    notifyListeners();
    runs++;
    try {
      final r = await run(exe, arguments(check: false), workingDirectory: projectDir);
      if (_disposed) return 'the project closed';
      _read(r);
    } catch (e) {
      if (_disposed) return 'the project closed';
      status = CodegenStatus.failed;
      error = 'could not run `fynkel codegen`: $e';
    }
    checkedAt = DateTime.now();
    generating = false;
    if (error != null && error!.contains('unknown argument')) {
      error =
          'the fynkel CLI at $exe is older than this editor ($error) — update it '
          '(`cargo install --path apps/cli` in the Fynkel checkout) or choose a newer one in Preferences';
    }
    notifyListeners();
    return status == CodegenStatus.upToDate ? null : error ?? 'fynkel codegen left the files stale';
  }

  /// Whether [path] is a script whose save regenerates: a `.dart` file
  /// under `lib/`, not the generated ones.
  bool isScriptSource(String path) {
    final lib = p.join(projectDir, 'lib');
    return p.isWithin(lib, path) &&
        path.endsWith('.dart') &&
        !path.endsWith('.g.dart') &&
        !p.isWithin(p.join(lib, 'generated'), path);
  }

  StreamSubscription<List<String>>? _saves;
  Timer? _recompile;

  /// Regenerates [delay] after a script under `lib/` changes (saved,
  /// added, deleted), then calls [reload] so the session re-reads the
  /// metadata. A save while a command runs waits for it.
  void recompileOnSave(
    Stream<List<String>> changes, {
    required void Function() reload,
    Duration delay = const Duration(milliseconds: 500),
  }) {
    _saves?.cancel();
    _saves = changes.listen((paths) {
      if (!paths.any(isScriptSource)) return;
      _recompile?.cancel();
      _recompile = Timer(delay, () => _recompileNow(reload, delay));
    });
  }

  Future<void> _recompileNow(void Function() reload, Duration delay) async {
    if (_disposed) return;
    if (status == CodegenStatus.running) {
      _recompile = Timer(delay, () => _recompileNow(reload, delay));
      return;
    }
    final problem = await generate();
    if (_disposed) return;
    if (problem == null) reload();
  }

  StreamSubscription<List<String>>? _assetChanges;
  Timer? _assetsTimer;

  /// Whether an `--assets-only` run is going.
  bool _assetsRunning = false;

  /// Assets changed while one ran: run once more after it.
  bool _assetsAgain = false;

  /// The command line [regenerateAssets] runs.
  List<String> assetsArguments() => ['codegen', '--project', projectDir, '--assets-only', '--json'];

  /// Keeps `lib/generated/assets.g.dart` (ADR-0026 §3) matching the
  /// project's assets: [delay] after anything under [assetsDir] is
  /// created, imported, renamed or deleted, runs `fynkel codegen
  /// --assets-only`, which scans the assets and rewrites the file only when
  /// it changed — no build_runner, so it is quick. Game code then names the
  /// new asset as `Assets.…` without anyone running codegen.
  void regenerateAssetsOn(
    Stream<List<String>> changes, {
    required String assetsDir,
    Duration delay = const Duration(milliseconds: 500),
  }) {
    _assetChanges?.cancel();
    _assetChanges = changes.listen((paths) {
      if (!paths.any((c) => p.isWithin(assetsDir, c))) return;
      _assetsTimer?.cancel();
      _assetsTimer = Timer(delay, () => unawaited(regenerateAssets()));
    });
  }

  /// One `fynkel codegen --project <dir> --assets-only --json`. Returns the
  /// problem, or null when the file was written or already up to date. A
  /// stale banner is checked again after a write, as the file may have
  /// been all it was about.
  Future<String?> regenerateAssets() async {
    if (!isDartPackage) return 'the project is not a Dart package (no pubspec.yaml)';
    final exe = cli();
    if (exe == null) return 'no `fynkel` CLI is configured (Preferences → Fynkel CLI)';
    if (_assetsRunning) {
      _assetsAgain = true;
      return null;
    }
    _assetsRunning = true;
    String? problem;
    String? outcome;
    try {
      final r = await run(exe, assetsArguments(), workingDirectory: projectDir);
      Map<String, Object?>? report;
      try {
        final decoded = jsonDecode(r.stdout.toString());
        if (decoded is Map<String, Object?>) report = decoded;
      } catch (_) {}
      final assets = report?['assets'];
      outcome = assets is Map ? assets['status']?.toString() : null;
      if (outcome == 'error') {
        problem = (assets as Map)['error']?.toString() ?? 'fynkel codegen --assets-only failed';
      } else if (outcome == null) {
        final err = r.stderr.toString().trim();
        problem = err.isNotEmpty ? err.split('\n').first : 'fynkel codegen --assets-only exited with ${r.exitCode}';
      }
    } catch (e) {
      problem = 'could not run `fynkel codegen --assets-only`: $e';
    }
    _assetsRunning = false;
    if (_disposed) return 'the project closed';
    if (_assetsAgain) {
      _assetsAgain = false;
      return regenerateAssets();
    }
    if (outcome == 'written' && status == CodegenStatus.stale) unawaited(check());
    return problem;
  }

  Process? _watcher;
  Completer<String?>? _waiting;

  /// Whether the build that answers [_waiting] has started.
  bool _waitingStarted = false;

  /// Whether `fynkel codegen --watch` is running for this project.
  bool get watching => _watcher != null;

  /// Keeps `fynkel codegen --watch` running: build_runner stays warm, so a
  /// saved script regenerates in about a second, and [reload] follows every
  /// build that wrote. A CLI that cannot watch (older than this editor), or
  /// a watch that ends, falls back to [recompileOnSave].
  Future<void> watch(Stream<List<String>> changes, {required void Function() reload}) async {
    final exe = cli();
    if (!isDartPackage || exe == null) return recompileOnSave(changes, reload: reload);
    final Process process;
    try {
      process = await start(exe, ['codegen', '--watch', '--project', projectDir], workingDirectory: projectDir);
    } catch (_) {
      if (!_disposed) recompileOnSave(changes, reload: reload);
      return;
    }
    if (_disposed) {
      unawaited(process.stdin.close());
      return;
    }
    _watcher = process;
    process.stdout
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen((line) => _onWatchLine(line, reload), onError: (Object _) {});
    process.stderr.listen((_) {}, onError: (Object _) {});
    unawaited(
      process.exitCode.then((_) {
        if (_disposed || !identical(_watcher, process)) return;
        _watcher = null;
        if (status == CodegenStatus.running) status = CodegenStatus.unknown;
        generating = false;
        _answer('the script watch ended');
        recompileOnSave(changes, reload: reload);
        notifyListeners();
      }),
    );
  }

  void _onWatchLine(String line, void Function() reload) {
    if (_disposed) return;
    final Object? event;
    try {
      event = jsonDecode(line);
    } catch (_) {
      return;
    }
    if (event is! Map) return;
    switch (event['event']) {
      case 'building':
        status = CodegenStatus.running;
        generating = true;
        if (_waiting != null) _waitingStarted = true;
      case 'built':
        status = CodegenStatus.upToDate;
        generating = false;
        error = null;
        stalePaths = const [];
        checkedAt = DateTime.now();
        runs++;
        if (event['status'] == 'written') reload();
        if (_waitingStarted) _answer(null);
      case 'failed':
        status = CodegenStatus.failed;
        generating = false;
        error = event['error']?.toString() ?? 'the build failed';
        checkedAt = DateTime.now();
        runs++;
        if (_waitingStarted) _answer(error);
      case 'exited':
        error = event['error']?.toString();
        return;
      default:
        return;
    }
    notifyListeners();
  }

  /// The answer of the next build to start: null when it built.
  Future<String?> _nextBuild() => (_waiting ??= Completer<String?>()).future;

  void _answer(String? problem) {
    final w = _waiting;
    _waiting = null;
    _waitingStarted = false;
    if (w != null && !w.isCompleted) w.complete(problem);
  }

  /// The project closed while a check ran: its answer is dropped.
  bool _disposed = false;

  @override
  void dispose() {
    _disposed = true;
    // The CLI ends the watch (and build_runner) when its stdin closes.
    final w = _watcher;
    _watcher = null;
    if (w != null) unawaited(w.stdin.close().catchError((Object _) {}));
    _answer('the project closed');
    _saves?.cancel();
    _recompile?.cancel();
    _assetChanges?.cancel();
    _assetsTimer?.cancel();
    super.dispose();
  }

  void _read(ProcessResult r) {
    final text = r.stdout.toString();
    Map<String, Object?>? report;
    try {
      final decoded = jsonDecode(text.trim().isEmpty ? '{}' : text);
      if (decoded is Map<String, Object?>) report = decoded;
    } catch (_) {
      report = null;
    }
    final project = report?['project'];
    if (project is Map) {
      switch (project['status']) {
        case 'upToDate':
        case 'written':
          status = CodegenStatus.upToDate;
          stalePaths = const [];
          return;
        case 'stale':
          status = CodegenStatus.stale;
          stalePaths = [for (final s in project['stale'] as List? ?? const []) s.toString()];
          return;
        case 'error':
          status = CodegenStatus.failed;
          error = project['error']?.toString() ?? 'the check failed';
          return;
      }
    }
    final native = report?['native'];
    if (native is Map && native['status'] == 'error') {
      status = CodegenStatus.failed;
      error = native['error']?.toString() ?? 'the native pipeline failed';
      return;
    }
    status = CodegenStatus.failed;
    final err = r.stderr.toString().trim();
    error = err.isNotEmpty ? err.split('\n').first : 'fynkel codegen exited with ${r.exitCode} and no report';
  }

  static String _quote(String s) => s.contains(' ') ? '"$s"' : s;
}
