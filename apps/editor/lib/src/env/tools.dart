// What the editor needs from the machine: the `fynkel` CLI (the only tool
// the editor shells out to — `fynkel create`, `fynkel codegen --check
// --json`, `fynkel run`), the Fynkel checkout the CLI's `create` needs, and
// Flutter. Nothing here touches a project; the launcher shows the result as
// `fynkel doctor`-style checks with a fix per failure.
import 'dart:convert';
import 'dart:io';

import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../prefs/preferences.dart';

/// Runs a process to completion — injectable so tests never spawn one.
typedef RunProcess = Future<ProcessResult> Function(
  String executable,
  List<String> arguments, {
  String? workingDirectory,
});

Future<ProcessResult> runProcess(String executable, List<String> arguments, {String? workingDirectory}) =>
    Process.run(executable, arguments, workingDirectory: workingDirectory);

/// One row of the launcher's checks.
final class DoctorCheck {
  const DoctorCheck({required this.title, required this.ok, required this.detail, this.fix, this.warning = false});

  final String title;
  final bool ok;

  /// What was found (a path, a version) or what went wrong.
  final String detail;

  /// What the user can do about a failure.
  final String? fix;

  /// A failure the editor works around (shown as a warning, not an error).
  final bool warning;
}

/// Resolves `name` on `PATH` (or returns it when it is an existing path).
String? which(String name, {Map<String, String>? environment}) {
  if (name.contains(p.separator) || name.startsWith('~')) {
    final expanded = name.startsWith('~')
        ? name.replaceFirst('~', (environment ?? Platform.environment)['HOME'] ?? '')
        : name;
    return File(expanded).existsSync() ? expanded : null;
  }
  final path = (environment ?? Platform.environment)['PATH'] ?? '';
  for (final dir in path.split(Platform.isWindows ? ';' : ':')) {
    if (dir.isEmpty) continue;
    final candidate = File(p.join(dir, name));
    if (candidate.existsSync()) return candidate.path;
  }
  return null;
}

/// Walks up from [start] to the Cargo workspace that contains `fynkel_native`
/// — the same rule the CLI uses (`apps/cli/src/schemas.rs`).
String? findFynkelCheckout(String start) {
  var dir = Directory(start).absolute.path;
  while (true) {
    final cargo = File(p.join(dir, 'Cargo.toml'));
    if (cargo.existsSync()) {
      final text = cargo.readAsStringSync();
      if (text.contains('[workspace]') && text.contains('fynkel_native')) return dir;
    }
    final parent = p.dirname(dir);
    if (parent == dir) return null;
    dir = parent;
  }
}

final class Toolchain {
  Toolchain({
    required this.fynkelCli,
    required this.fynkelRoot,
    required this.flutter,
    required this.flutterVersion,
    required this.ideExecutable,
    required this.ideCommand,
    required this.abiVersion,
    required this.abiError,
  });

  /// Everything unknown — what the launcher shows before detection finishes.
  Toolchain.empty()
    : this(
        fynkelCli: null,
        fynkelRoot: null,
        flutter: null,
        flutterVersion: null,
        ideExecutable: null,
        ideCommand: '',
        abiVersion: null,
        abiError: null,
      );

  final String? fynkelCli;
  final String? fynkelRoot;
  final String? flutter;
  final String? flutterVersion;
  final String? ideExecutable;
  final String ideCommand;

  /// The authoring ABI version the loaded runtime reports, or null with [abiError].
  final int? abiVersion;
  final String? abiError;

  bool get canCreate => fynkelCli != null && fynkelRoot != null;

  /// Order of discovery for the CLI: the preference, `$FYNKEL_CLI`, then
  /// the newest of `fynkel` on `PATH` and `target/{release,debug}/fynkel`
  /// of the checkout that contains the running editor or the working
  /// directory (development) — an installed CLI older than the checkout's
  /// build would lack what this editor asks of it.
  static Future<Toolchain> detect({
    required Preferences prefs,
    Map<String, String>? environment,
    RunProcess run = runProcess,
    String? executablePath,
    String? workingDirectory,
    bool probeAbi = true,
  }) async {
    final env = environment ?? Platform.environment;
    final starts = [executablePath ?? Platform.resolvedExecutable, workingDirectory ?? Directory.current.path];
    String? root = prefs.fynkelRoot ?? env['FYNKEL_ROOT'];
    if (root != null && !Directory(root).existsSync()) root = null;
    root ??= starts.map(findFynkelCheckout).whereType<String>().firstOrNull;

    String? cli;
    for (final candidate in [prefs.fynkelCli, env['FYNKEL_CLI']]) {
      if (candidate != null && File(candidate).existsSync()) {
        cli = candidate;
        break;
      }
    }
    if (cli == null) {
      final found = [
        ?which('fynkel', environment: env),
        if (root != null)
          for (final profile in ['release', 'debug'])
            if (File(p.join(root, 'target', profile, 'fynkel')).existsSync()) p.join(root, 'target', profile, 'fynkel'),
      ];
      DateTime modified(String f) => File(f).statSync().modified;
      // The first found wins a tie: PATH, then release, then debug.
      for (final f in found) {
        if (cli == null || modified(f).isAfter(modified(cli))) cli = f;
      }
    }
    // A CLI found on PATH still tells us where its checkout is.
    if (root == null && cli != null) root = findFynkelCheckout(p.dirname(cli));

    final flutter = which(prefs.flutterCommand, environment: env);
    String? flutterVersion;
    if (flutter != null) {
      try {
        final r = await run(flutter, ['--version', '--machine']);
        if (r.exitCode == 0) {
          final decoded = jsonDecode(r.stdout as String);
          if (decoded is Map) flutterVersion = decoded['frameworkVersion'] as String?;
        }
      } catch (_) {
        flutterVersion = null;
      }
    }

    final ideExe = ideExecutableOf(prefs.ideCommand);
    int? abi;
    String? abiError;
    if (probeAbi) {
      try {
        abi = AuthoringSession.loadedAbiVersion;
      } catch (e) {
        abiError = '$e';
      }
    }
    return Toolchain(
      fynkelCli: cli,
      fynkelRoot: root,
      flutter: flutter,
      flutterVersion: flutterVersion,
      ideExecutable: ideExe == null ? null : which(ideExe, environment: env),
      ideCommand: prefs.ideCommand,
      abiVersion: abi,
      abiError: abiError,
    );
  }

  /// The executable named by an IDE command template.
  static String? ideExecutableOf(String template) => splitCommand(template).firstOrNull;

  List<DoctorCheck> get checks => [
    DoctorCheck(
      title: 'Fynkel CLI',
      ok: fynkelCli != null,
      detail: fynkelCli ?? 'not found on PATH, in \$FYNKEL_CLI or under a checkout\'s target/',
      fix: fynkelCli == null
          ? 'Build it with `cargo build -p fynkel` in the Fynkel checkout, or set its path in Preferences.'
          : null,
    ),
    DoctorCheck(
      title: 'Fynkel checkout',
      ok: fynkelRoot != null,
      detail: fynkelRoot ?? 'not found; New Project needs it (package:fynkel is a path dependency)',
      fix: fynkelRoot == null ? 'Set the checkout directory in Preferences (or \$FYNKEL_ROOT).' : null,
    ),
    DoctorCheck(
      title: 'Flutter',
      ok: flutter != null,
      detail: flutter == null
          ? '`$ideCommandFlutterName` not found on PATH'
          : '$flutter${flutterVersion == null ? '' : ' ($flutterVersion)'}',
      fix: flutter == null ? 'Install Flutter and put it on PATH, or set the command in Preferences.' : null,
      warning: true,
    ),
    DoctorCheck(
      title: 'Authoring ABI',
      ok: abiVersion == authoringAbiVersion,
      detail:
          abiError ??
          (abiVersion == null ? 'not probed' : 'runtime speaks v$abiVersion, editor expects v$authoringAbiVersion'),
      fix: abiVersion == authoringAbiVersion
          ? null
          : 'Rebuild the editor (`flutter run`) so package:fynkel\'s hook rebuilds the runtime.',
    ),
    DoctorCheck(
      title: 'IDE command',
      ok: ideExecutable != null,
      detail: ideExecutable == null
          ? '`${ideExecutableOf(ideCommand) ?? ideCommand}` not found on PATH'
          : '$ideCommand → $ideExecutable',
      fix: ideExecutable == null ? 'Pick an IDE in Preferences; Console rows open there.' : null,
      warning: true,
    ),
  ];

  String get ideCommandFlutterName => 'flutter';

  bool get allOk => checks.every((c) => c.ok);
}

/// Splits a command line on whitespace, honouring single and double quotes.
List<String> splitCommand(String command) {
  final out = <String>[];
  final buf = StringBuffer();
  String? quote;
  var inToken = false;
  for (final ch in command.runes.map(String.fromCharCode)) {
    if (quote != null) {
      if (ch == quote) {
        quote = null;
      } else {
        buf.write(ch);
      }
    } else if (ch == '"' || ch == "'") {
      quote = ch;
      inToken = true;
    } else if (ch.trim().isEmpty) {
      if (inToken) {
        out.add(buf.toString());
        buf.clear();
        inToken = false;
      }
    } else {
      buf.write(ch);
      inToken = true;
    }
  }
  if (inToken) out.add(buf.toString());
  return out;
}

/// Opens a file at a line in the user's IDE from the preferences' template.
final class IdeLauncher {
  IdeLauncher(this.prefs, {this.start = Process.start, Map<String, String>? environment}) : env = environment;

  final Preferences prefs;
  final Future<Process> Function(String executable, List<String> arguments) start;
  final Map<String, String>? env;

  /// The argument list for [file] at [line]:[column] under the current template.
  List<String> arguments(String file, {int? line, int? column}) {
    final parts = splitCommand(prefs.ideCommand);
    final substituted = [
      for (final part in parts)
        part.replaceAll('{file}', file).replaceAll('{line}', '${line ?? 1}').replaceAll('{column}', '${column ?? 1}'),
    ];
    if (!prefs.ideCommand.contains('{file}')) substituted.add(file);
    return substituted;
  }

  /// Null on success, otherwise the reason (shown in the Console's status).
  Future<String?> open(String file, {int? line, int? column}) async {
    final args = arguments(file, line: line, column: column);
    if (args.isEmpty) return 'the IDE command is empty (Preferences)';
    final exe = which(args.first, environment: env);
    if (exe == null) return '`${args.first}` was not found on PATH (Preferences → IDE command)';
    try {
      await start(exe, args.sublist(1));
      return null;
    } catch (e) {
      return 'could not start `${args.first}`: $e';
    }
  }
}

/// Reveals a directory or file in the platform's file browser.
Future<String?> revealInFileBrowser(String path, {RunProcess run = runProcess}) async {
  try {
    final r = Platform.isMacOS
        ? await run('open', ['-R', path])
        : Platform.isWindows
        ? await run('explorer', ['/select,', path])
        : await run('xdg-open', [p.dirname(path)]);
    return r.exitCode == 0 ? null : 'exit ${r.exitCode}: ${r.stderr}';
  } catch (e) {
    return '$e';
  }
}
