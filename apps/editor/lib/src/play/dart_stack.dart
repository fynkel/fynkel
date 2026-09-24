// A Dart stack trace read back into frames (roadmap Phase 2, IDE
// navigation): the game's uncaught errors reach the Console with their
// stack (`EditorChannel.error`), and a frame in the game's own code opens
// the IDE at its line — Unity's click on a Console stack line.
//
// Both forms Dart prints are read: the VM's
// `#0      Player.update (package:game/player.dart:42:7)` and the terse one
// of `package:stack_trace` (`package:game/player.dart 42:7  Player.update`).
import 'dart:io';

import 'package:path/path.dart' as p;

/// One frame of a stack.
final class DartFrame {
  const DartFrame({required this.member, required this.uri, this.line, this.column});

  /// `Player.update`, `<fn>`, `<asynchronous suspension>`-free.
  final String member;

  /// `package:game/player.dart`, `file:///…/main.dart`, `dart:core/…`.
  final Uri uri;
  final int? line;
  final int? column;

  /// `package:game/player.dart:42:7`.
  String get location => '$uri${line == null ? '' : ':$line'}${column == null ? '' : ':$column'}';

  /// The file on disk when the frame is the project's own code: a
  /// `package:<own>/…` under `<project>/lib`, or a `file:` inside the
  /// project. Null for the SDK, Flutter, the engine and other packages.
  String? sourceIn(String projectDir, {String? ownPackage}) {
    if (uri.scheme == 'package') {
      final segs = uri.pathSegments;
      if (ownPackage == null || segs.length < 2 || segs.first != ownPackage) return null;
      return p.joinAll([projectDir, 'lib', ...segs.skip(1)]);
    }
    if (uri.scheme == 'file') {
      final path = p.normalize(uri.toFilePath());
      return p.isWithin(projectDir, path) ? path : null;
    }
    return null;
  }
}

final _vm = RegExp(r'^#\d+\s+(.+?)\s+\(((?:package|file|dart|org-dartlang-sdk):[^)]*?)(?::(\d+))?(?::(\d+))?\)$');
final _terse = RegExp(r'^((?:package|file|dart):\S+)\s+(\d+)(?::(\d+))?\s+(.+)$');

/// The frames of [stack], in order; lines that are not frames (the
/// `<asynchronous suspension>` markers, a message) are skipped.
List<DartFrame> parseDartStack(String? stack) {
  if (stack == null || stack.isEmpty) return const [];
  final frames = <DartFrame>[];
  for (final raw in stack.split('\n')) {
    final line = raw.trim();
    // `flutter run` prefixes the app's lines (`flutter: #0 …`).
    final text = line.startsWith('flutter: ') ? line.substring(9).trim() : line;
    final vm = _vm.firstMatch(text);
    if (vm != null) {
      final uri = Uri.tryParse(vm[2]!);
      if (uri != null) {
        frames.add(
          DartFrame(member: vm[1]!, uri: uri, line: int.tryParse(vm[3] ?? ''), column: int.tryParse(vm[4] ?? '')),
        );
      }
      continue;
    }
    final terse = _terse.firstMatch(text);
    if (terse != null) {
      final uri = Uri.tryParse(terse[1]!);
      if (uri != null) {
        frames.add(
          DartFrame(member: terse[4]!, uri: uri, line: int.tryParse(terse[2]!), column: int.tryParse(terse[3] ?? '')),
        );
      }
    }
  }
  return frames;
}

/// The `name:` of the pubspec in [projectDir] — the package a game's own
/// frames are in — or null.
String? packageNameOf(String projectDir) {
  final f = File(p.join(projectDir, 'pubspec.yaml'));
  if (!f.existsSync()) return null;
  final m = RegExp(r'^name:\s*([A-Za-z0-9_]+)\s*$', multiLine: true).firstMatch(f.readAsStringSync());
  return m?[1];
}
