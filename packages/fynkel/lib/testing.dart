/// Test-time helpers for games on `package:fynkel`.
///
/// The runtime reads a project's cooked cache only (`World.loadProject`
/// opens `<project>/.fynkel/cache`), so a test that loads a project must
/// cook it first — `fynkel run` and `fynkel test` do that for you; a plain
/// `dart test`/`flutter test` does not. [cookProject] runs the `fynkel`
/// CLI for you, usually once in `setUpAll`, and returns the cache
/// directory to hand to `World.loadProject(dir, cacheDir: ...)`.
library;

import 'dart:convert';
import 'dart:io';

/// Cooks the project at [projectDir] (its `fynkel.project.json`) into
/// [cacheDir] (default `<projectDir>/.fynkel/cache`) with the `fynkel` CLI
/// and returns the cache directory. A warm cache costs milliseconds.
///
/// The CLI is found through `FYNKEL_CLI` (a path to the executable),
/// `fynkel` on `PATH`, or — inside a Fynkel source checkout, which is
/// where `package:fynkel` resolves to when the game depends on it by path
/// — `cargo run -p fynkel_cli`. Throws [ProcessException] when none is
/// found, and [StateError] with the CLI's output when the cook fails
/// (an asset that does not cook, a project with errors).
String cookProject(String projectDir, {String? cacheDir, bool force = false}) {
  final cache = cacheDir ?? '$projectDir/.fynkel/cache';
  final args = ['cook', '--project', projectDir, '--cache', cache, if (force) '--force'];
  final result = runFynkel(args);
  if (result.exitCode != 0) {
    throw StateError('fynkel ${args.join(' ')} failed (exit ${result.exitCode}):\n${result.stdout}${result.stderr}');
  }
  return cache;
}

/// Runs the `fynkel` CLI with [args] and returns its result (see
/// [cookProject] for how it is found).
ProcessResult runFynkel(List<String> args) {
  final cli = fynkelCli();
  if (cli != null) return Process.runSync(cli, args);
  final checkout = fynkelCheckout();
  if (checkout == null) {
    throw ProcessException(
      'fynkel',
      args,
      'no `fynkel` CLI: set FYNKEL_CLI to the executable, put `fynkel` on PATH, '
          'or depend on package:fynkel from a Fynkel source checkout (cargo run -p fynkel_cli)',
    );
  }
  final cargo = _findCargo();
  if (cargo == null) {
    throw ProcessException('cargo', args, 'no `cargo` on PATH or in ~/.cargo/bin to build the fynkel CLI');
  }
  return Process.runSync(
    cargo,
    ['run', '-q', '-p', 'fynkel_cli', '--manifest-path', '${checkout.toFilePath()}Cargo.toml', '--', ...args],
    environment: {'PATH': '${Platform.environment['PATH'] ?? ''}:${File(cargo).parent.path}'},
  );
}

/// The `fynkel` executable from `FYNKEL_CLI` or `PATH`, or null.
String? fynkelCli() {
  final explicit = Platform.environment['FYNKEL_CLI'];
  if (explicit != null && explicit.isNotEmpty) return explicit;
  final exe = Platform.isWindows ? 'fynkel.exe' : 'fynkel';
  return _onPath(exe);
}

/// The Fynkel source checkout `package:fynkel` lives in (the Cargo
/// workspace two directories above the package), or null for a published
/// package. Resolved through the package config of the current package
/// (`.dart_tool/package_config.json`, found from the working directory
/// upwards), which both `dart test` and `flutter test` leave in place.
Uri? fynkelCheckout() {
  final package = _packageRoot('fynkel');
  if (package == null) return null;
  final root = package.resolve('../../');
  final manifest = File.fromUri(root.resolve('Cargo.toml'));
  if (!manifest.existsSync()) return null;
  final text = manifest.readAsStringSync();
  return text.contains('[workspace]') && text.contains('fynkel_native') ? root : null;
}

Uri? _packageRoot(String name) {
  var dir = Directory.current.absolute;
  while (true) {
    final config = File('${dir.path}/.dart_tool/package_config.json');
    if (config.existsSync()) {
      final json = jsonDecode(config.readAsStringSync()) as Map<String, Object?>;
      for (final p in (json['packages'] as List?) ?? const []) {
        final entry = (p as Map).cast<String, Object?>();
        if (entry['name'] != name) continue;
        final root = config.absolute.uri.resolve(entry['rootUri'] as String);
        return root.path.endsWith('/') ? root : Uri.parse('$root/');
      }
      return null;
    }
    final parent = dir.parent;
    if (parent.path == dir.path) return null;
    dir = parent;
  }
}

String? _findCargo() {
  final home = Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'];
  final exe = Platform.isWindows ? 'cargo.exe' : 'cargo';
  return _onPath(exe, extra: [if (home != null) '$home/.cargo/bin']);
}

String? _onPath(String exe, {List<String> extra = const []}) {
  final sep = Platform.isWindows ? ';' : ':';
  final dirs = [...?Platform.environment['PATH']?.split(sep), ...extra];
  for (final d in dirs) {
    if (d.isEmpty) continue;
    final candidate = '$d${Platform.pathSeparator}$exe';
    if (File(candidate).existsSync()) return candidate;
  }
  return null;
}
