import 'dart:convert';
import 'dart:io';

/// What the command says when it can't go on.
class CliException implements Exception {
  /// It can't, for the reason in [message].
  CliException(this.message);

  /// The reason.
  final String message;
  @override
  String toString() => message;
}

/// The programs the command runs, and the machine it is on; a test's own
/// stands in for it.
class Tools {
  /// The environment.
  Map<String, String> get env => Platform.environment;

  /// Whether this is a Mac.
  bool get isMac => Platform.isMacOS;

  /// The folder the command was run in.
  Directory get here => Directory.current;

  /// Runs [program] and gives what it printed; null if there is none, or
  /// it failed.
  Future<String?> read(String program, List<String> args) async {
    try {
      final result = await Process.run(program, args);
      return result.exitCode == 0 ? '${result.stdout}' : null;
    } on ProcessException {
      return null;
    }
  }

  /// Runs [program] in [folder], and throws what it said if it fails.
  Future<void> run(String program, List<String> args, {Directory? folder}) async {
    final ProcessResult result;
    try {
      result = await Process.run(program, args, workingDirectory: folder?.path);
    } on ProcessException {
      throw CliException('`$program` was not found. Install Flutter: https://docs.flutter.dev/get-started');
    }
    if (result.exitCode != 0) {
      throw CliException('`$program ${args.join(' ')}` failed:\n${'${result.stdout}${result.stderr}'.trim()}');
    }
  }

  /// Flutter's version and its Dart's, or null without Flutter.
  Future<({String flutter, String dart})?> flutterVersion() async {
    final text = await read('flutter', ['--version', '--machine']);
    if (text == null) return null;
    try {
      // Flutter may say something else first (a new version, its welcome).
      final json = jsonDecode(text.substring(text.indexOf('{'), text.lastIndexOf('}') + 1)) as Map<String, Object?>;
      return (flutter: json['frameworkVersion']! as String, dart: (json['dartSdkVersion']! as String).split(' ').first);
    } on Object {
      return null;
    }
  }
}

/// The oldest Flutter that has what Fynkel needs.
const oldestFlutter = '3.47.0';

/// Whether version [a] is [b] or newer, by its numbers ("3.50.0-1.pre" is
/// taken as 3.50.0).
bool atLeast(String a, String b) {
  List<int> parse(String v) => v.split(RegExp('[-+]')).first.split('.').map((part) => int.tryParse(part) ?? 0).toList();
  final (x, y) = (parse(a), parse(b));
  for (var i = 0; i < y.length; i++) {
    final xi = i < x.length ? x[i] : 0;
    if (xi != y[i]) return xi > y[i];
  }
  return true;
}
