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
// editor runs the generating command itself only on an explicit action —
// Create → Behaviour ([generate], ADR-0016 amendment of 2026-09-24): the
// CLI as a subprocess, never game code in the editor process.
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

final class CodegenCheck extends ChangeNotifier {
  CodegenCheck({required this.projectDir, required this.run, required this.cli, required this.repoRoot});

  /// The game project (the Dart package with `pubspec.yaml`).
  final String projectDir;
  final RunProcess run;

  /// The `fynkel` executable, or null when none is configured.
  final String? Function() cli;

  /// The Fynkel checkout the CLI's native pipeline needs (`--root`), or null.
  final String? Function() repoRoot;

  CodegenStatus status = CodegenStatus.unknown;
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
    status = CodegenStatus.running;
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
    if (error != null && error!.contains('unknown argument')) {
      error =
          'the fynkel CLI at $exe is older than this editor ($error) — update it '
          '(`cargo install --path apps/cli` in the Fynkel checkout) or choose a newer one in Preferences';
    }
    notifyListeners();
    return status == CodegenStatus.upToDate ? null : error ?? 'fynkel codegen left the files stale';
  }

  /// The project closed while a check ran: its answer is dropped.
  bool _disposed = false;

  @override
  void dispose() {
    _disposed = true;
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
